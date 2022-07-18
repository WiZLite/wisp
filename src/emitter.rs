use crate::{
    env::{Env, Pointer, Variable},
    parser::{parse_source, AST, TypeAST},
    resolver::{dissolve_type, resolve_type, Type, TypeEnv},
};
use anyhow::{anyhow, bail, ensure, Result, Context};
use std::{collections::HashMap, hash::Hash, rc::Rc, cell::RefCell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Func,
}

#[derive(PartialEq, Eq, Debug, Clone, Copy, Hash)]
pub enum PrimitiveType {
    I32 = 0x7F,
    F32 = 0x6F,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Export {
    pub export_type: ExportKind,
    pub name: String,
    pub func_index: u32,
}

#[derive(PartialEq, Eq, Debug, Clone, Copy, Hash)]
pub enum SignatureType {
    Func = 0x60,
}

#[derive(PartialEq, Eq, Debug, Hash, Clone)]
pub struct Signature {
    pub sig_type: SignatureType,
    pub params: Vec<PrimitiveType>,
    pub results: Vec<PrimitiveType>,
}

#[derive(Debug, PartialEq)]
pub struct Function {
    pub signature_index: u32,
    pub arg_types: Vec<Rc<Type>>,
    pub result_type: Rc<Type>,
    pub body: Vec<OpCode>,
}

#[derive(PartialEq, Debug)]
pub enum OpCode {
    Drop,
    End,
    LocalDeclCount(u8),
    LocalGet(u8),
    Call(u32),
    I32Const(i32),
    F32Const(f32),
    I32Add,
    I32Sub,
    I32Mul,
    I32Div,
    F32Add,
    F32Sub,
    F32Mul,
    F32Div,
    F32Neg,
    F32ConvertI32S,
}

#[derive(Debug, PartialEq, Default)]
pub struct Module {
    pub signatures: HashMap<Signature, u16>,
    pub exports: Vec<Export>,
    pub functions: Rc<RefCell<HashMap<String, (usize, Function)>>>,
}

pub struct Emitter<'a> {
    module: &'a mut Module,
}

#[derive(Debug)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug)]
enum UnaryOp {
    Minus,
}

impl<'a> Emitter<'a> {
    pub fn new(module: &'a mut Module) -> Self {
        Emitter { module }
    }
    fn emit_unary_exp(
        &mut self,
        codes: &mut Vec<OpCode>,
        op: UnaryOp,
        operand: &AST,
        env: &mut Env,
    ) -> Result<Type> {
        let mut temp_vec = Vec::new();
        let t = self.emit_obj(&mut temp_vec, operand, env)?;
        match op {
            UnaryOp::Minus => match *t {
                Type::F32 => {
                    temp_vec.push(OpCode::F32Neg);
                    codes.append(&mut temp_vec);
                    Ok(Type::F32)
                }
                Type::I32 => {
                    codes.push(OpCode::I32Const(0));
                    codes.append(&mut temp_vec);
                    codes.push(OpCode::I32Sub);
                    Ok(Type::I32)
                },
                _ => bail!("Unary expression: {:?} accepts only numeric types, but found {:?}", op, operand)
            },
        }
    }
    fn emit_bin_exp(
        &mut self,
        codes: &mut Vec<OpCode>,
        op: BinOp,
        lhs: &AST,
        rhs: &AST,
        env: &mut Env,
    ) -> Result<Type> {
        let mut lhs_temp_vec = Vec::new();
        let mut rhs_temp_vec = Vec::new();
        let lhs_type = self.emit_obj(&mut lhs_temp_vec, lhs, env)?;
        let rhs_type = self.emit_obj(&mut rhs_temp_vec, rhs, env)?;
        let unsupported_msg = "";
        let (opcode, result_type) = match op {
            BinOp::Add => match *lhs_type {
                Type::I32 => match *rhs_type {
                    Type::F32 => {
                        // cast lhs to f32
                        lhs_temp_vec.push(OpCode::F32ConvertI32S);
                        (OpCode::F32Add, Type::F32)
                    }
                    Type::I32 => (OpCode::I32Add, Type::I32),
                    _ => bail!("binary expression: {:?} accepts only numeric types, but second argument type is {:?}.", op, rhs_type)
                },
                Type::F32 => match *rhs_type {
                    Type::I32 => {
                        // cast rhs to f32
                        rhs_temp_vec.push(OpCode::F32ConvertI32S);
                        (OpCode::F32Add, Type::F32)
                    }
                    Type::F32 => (OpCode::F32Add, Type::F32),
                    _ => bail!("binary expression: {:?} accepts only numeric types, but second argument type is {:?}.", op, rhs_type)
                },
                _ => bail!(unsupported_msg)
            },
            BinOp::Sub => match *lhs_type {
                Type::I32 => match *rhs_type {
                    Type::F32 => {
                        // cast lhs to f32
                        lhs_temp_vec.push(OpCode::F32ConvertI32S);
                        (OpCode::F32Sub, Type::F32)
                    }
                    Type::I32 => (OpCode::I32Sub, Type::I32),
                    _ => bail!("binary expression: {:?} accepts only numeric types, but second argument type is {:?}.", op, rhs_type)
                },
                Type::F32 => match *rhs_type {
                    Type::I32 => {
                        // cast rhs to f32
                        rhs_temp_vec.push(OpCode::F32ConvertI32S);
                        (OpCode::F32Sub, Type::F32)
                    }
                    Type::F32 => (OpCode::F32Sub, Type::F32),
                    _ => bail!("binary expression: {:?} accepts only numeric types, but second argument type is {:?}.", op, rhs_type)
                },
                _ => bail!("binary expression: {:?} accepts only numeric types, but first argument type is {:?}.", op, lhs_type)
            },
            BinOp::Mul => match *lhs_type {
                Type::I32 => match *rhs_type {
                    Type::F32 => {
                        // cast lhs to f32
                        lhs_temp_vec.push(OpCode::F32ConvertI32S);
                        (OpCode::F32Mul, Type::F32)
                    }
                    Type::I32 => (OpCode::I32Mul, Type::I32),
                    _ => bail!("binary expression: {:?} accepts only numeric types, but second argument type is {:?}.", op, rhs_type)
                },
                Type::F32 => match *rhs_type {
                    Type::F32 => (OpCode::F32Mul, Type::F32),
                    Type::I32 => {
                        rhs_temp_vec.push(OpCode::F32ConvertI32S);
                        (OpCode::F32Mul, Type::F32)
                    }
                    _ => bail!("binary expression: {:?} accepts only numeric types, but second argument type is {:?}.", op, rhs_type)
                },
                _ => bail!("binary expression: {:?} accepts only numeric types, but first argument type is {:?}.", op, lhs_type)
            },
            BinOp::Div => match *lhs_type {
                Type::I32 => match *rhs_type {
                    Type::F32 => {
                        // cast lhs to f32
                        lhs_temp_vec.push(OpCode::F32ConvertI32S);
                        (OpCode::F32Div, Type::F32)
                    }
                    Type::I32 => (OpCode::I32Div, Type::I32),
                    _ => bail!("binary expression: {:?} accepts only numeric types, but second argument type is {:?}.", op, rhs_type)
                },
                Type::F32 => match *rhs_type {
                    Type::F32 => (OpCode::F32Div, Type::F32),
                    Type::I32 => {
                        rhs_temp_vec.push(OpCode::F32ConvertI32S);
                        (OpCode::F32Div, Type::F32)
                    }
                    _ => bail!("binary expression: {:?} accepts only numeric types, but second argument type is {:?}.", op, rhs_type)
                },
                _ => bail!("binary expression: {:?} accepts only numeric types, but first argument type is {:?}.", op, lhs_type)
            },
        };
        codes.append(&mut lhs_temp_vec);
        codes.append(&mut rhs_temp_vec);
        codes.push(opcode);
        Ok(result_type)
    }
    fn emit_function_call(
        &mut self,
        codes: &mut Vec<OpCode>,
        index: u32,
        func: &Function,
        args: &[AST],
        env: &mut Env,
    ) -> Result<Rc<Type>> {
        for arg in args {
            self.emit_obj(codes, arg, env)?;
        }
        codes.push(OpCode::Call(index as u32));
        Ok(func.result_type.clone())
    }
    fn emit_list(&mut self, codes: &mut Vec<OpCode>, ast: &AST, env: &mut Env) -> Result<Rc<Type>> {
        match ast {
            AST::List(list) => {
                let first = &list[0];
                Ok(match first {
                    AST::Add | AST::Mul | AST::Div => {
                        ensure!(
                            list.len() == 3,
                            "Binary op can only be evaluated with 2 args"
                        );
                        let result_type = self.emit_bin_exp(
                            codes,
                            match first {
                                AST::Add => BinOp::Add,
                                AST::Mul => BinOp::Mul,
                                AST::Div => BinOp::Div,
                                _ => unreachable!(),
                            },
                            &list[1],
                            &list[2],
                            env,
                        )?;
                        Rc::new(result_type)
                    }
                    AST::Sub => match &list.len() {
                        2 => {
                            let result_type =
                                self.emit_unary_exp(codes, UnaryOp::Minus, &list[1], env)?;
                            Rc::new(result_type)
                        }
                        3 => {
                            let result_type =
                                self.emit_bin_exp(codes, BinOp::Sub, &list[1], &list[2], env)?;
                            Rc::new(result_type)
                        }
                        _ => bail!("- must be called with one or two arguments"),
                    },
                    AST::Symbol(name) => {
                        match *name {
                            "let" => {
                                todo!()
                            }
                            _ => { // emit function call
                                let module_functions = self.module.functions.clone();
                                let module_func_refmut = module_functions.borrow_mut();
                                let (index, func) = module_func_refmut.get(*name).with_context(|| format!("Unable to find function {:?}", &name))?;
                                self.emit_function_call(codes, *index as u32, func, &list[1..], env)?
                            }
                        }
                    }
                    _ => todo!(
                        "Only [+, -, *, /] operators and function call can be emitted for now."
                    ),
                })
            }
            _ => bail!("Invalid argument. emit_list only accepts AST::List"),
        }
    }
    fn emit_obj(&mut self, codes: &mut Vec<OpCode>, ast: &AST, env: &mut Env) -> Result<Rc<Type>> {
        match ast {
            AST::List(_) => return self.emit_list(codes, ast, env),
            // TODO: Infer type
            AST::NumberLiteral(literal) => {
                if let Ok(i32_val) = literal.parse::<i32>() {
                    codes.push(OpCode::I32Const(i32_val));
                    return Ok(Rc::new(Type::I32));
                } else if let Ok(f32_val) = literal.parse::<f32>() {
                    codes.push(OpCode::F32Const(f32_val));
                    return Ok(Rc::new(Type::F32));
                } else {
                    bail!("Failed to parse number");
                }
            }
            AST::Symbol(name) => match env.get(name) {
                None => bail!("Symbol {} not found in this scope", name),
                Some(variable) => match variable.pointer {
                    Pointer::Local(index) => {
                        codes.push(OpCode::LocalGet(index));
                        return Ok(variable.t.clone());
                    }
                },
            },
            _ => todo!(),
        }
    }
    fn emit_func(&mut self, ast: &AST, env: Rc<Env>) -> Result<()> {
        if let AST::List(func_list) = ast {
            let mut slice = &func_list[..];
            let (is_export, name, result_type_ast, args, forms) = match func_list[0] {
                AST::Symbol(s) => {
                    let is_export = if s == "export" {
                        ensure!(
                            slice[1] == AST::Symbol("defn"),
                            "Failed to compile function. 'defn' is expected after 'export'"
                        );
                        slice = &slice[2..];
                        true
                    } else {
                        ensure!(s == "defn", "Failed to compile function. func list must start with 'export' or 'defn'");
                        slice = &slice[1..];
                        false
                    };
                    let (name, type_ast) = match &slice[0] {
                        AST::SymbolWithAnnotation(s, type_ast) => (*s, type_ast),
                        AST::Symbol(s) => (*s, &TypeAST::Unit),
                        _ => bail!("A symbol with type annotaion is expected after 'defn'"),
                    };
                    let mut args = Vec::new();
                    match &slice[1] {
                        AST::Vector(list) => {
                            for arg in list {
                                args.push(match arg {
                                    AST::SymbolWithAnnotation(name, type_ast) => (*name, type_ast),
                                    _ => bail!(
                                        "Function argument should be a symbol annotated with ':'"
                                    ),
                                });
                            }
                        }
                        _ => bail!("Function args vector is required after 'defn'"),
                    };
                    let forms = Vec::from(&slice[2..]);
                    (is_export, name, type_ast, args, forms)
                }
                _ => todo!(),
            };

            // TODO: Impl type symbol functionality
            let empty_type_env = TypeEnv::default();

            let func_index = self.module.functions.borrow().len();
            let mut new_env = Env::extend(env.clone());
            let mut local_index = 0;
            for arg in &args {
                new_env.set(
                    arg.0,
                    Variable {
                        pointer: Pointer::Local(local_index),
                        t: resolve_type(arg.1, &empty_type_env),
                    },
                );
                local_index += 1;
            }

            // Resolve arg types and func return type
            let arg_types = args
                .iter()
                .map(|(_, type_ast)| resolve_type(type_ast, &empty_type_env))
                .collect::<Vec<_>>();
            let result_type = resolve_type(result_type_ast, &empty_type_env);

            // TODO: local variables
            let mut func_body = Vec::new();
            func_body.push(OpCode::LocalDeclCount(0));

            for (index, form) in forms.iter().enumerate() {
                let last = index == forms.len() - 1;
                if last {
                    let last_form_type = self.emit_obj(&mut func_body, &form, &mut new_env)?;
                    if *result_type == Type::Unit {
                        let stack_cnt = dissolve_type(last_form_type).len();
                        // Drop unused results
                        for _ in 0..stack_cnt {
                            func_body.push(OpCode::Drop);
                        }
                    }
                    else if *last_form_type != *result_type {
                        bail!("mismatched return type. Expected `{:?}`, but found `{:?}`", result_type_ast, last_form_type)
                    }
                } else {
                    let emitted_type = self.emit_obj(&mut func_body, &form, &mut new_env)?;
                    let stack_cnt = dissolve_type(emitted_type).len();
                    // Drop unused results
                    for _ in 0..stack_cnt {
                        func_body.push(OpCode::Drop);
                    }
                }
            }
            func_body.push(OpCode::End);

            let signature = Signature {
                sig_type: SignatureType::Func,
                params: arg_types
                    .iter()
                    .flat_map(|types| dissolve_type(types.clone()))
                    .collect::<Vec<_>>(),
                results: dissolve_type(resolve_type(result_type_ast, &empty_type_env)),
            };
            let signature_index = match self.module.signatures.get(&signature) {
                Some(index) => *index,
                None => {
                    let index = self.module.signatures.len() as u16;
                    self.module.signatures.insert(signature.clone(), index);
                    index
                }
            };

            self.module.functions.borrow_mut().insert(
                name.to_string(),
                (
                    func_index,
                    Function {
                        arg_types,
                        result_type,
                        signature_index: signature_index as u32,
                        body: func_body,
                    },
                ),
            );

            if is_export {
                self.module.exports.push(Export {
                    export_type: ExportKind::Func,
                    name: name.to_string(),
                    func_index: func_index as u32,
                });
            }
        } else {
            bail!("Invalid argument.");
        }
        Ok(())
    }
    fn emit_toplevel(&mut self, ast: &AST) -> Result<()> {
        // TODO: Impl Global Variables
        // toplevel can only be a function for now.
        self.emit_func(ast, Rc::new(Env::default()))
    }
    fn emit_module(&mut self, ast: &AST) -> Result<()> {
        let toplevels = match ast {
            AST::Module(tops) => tops,
            _ => return Err(anyhow!("Invalid argument.")),
        };
        for toplevel in toplevels {
            self.emit_toplevel(toplevel)?;
        }
        Ok(())
    }
    pub fn emit(&mut self, source: &str) -> Result<()> {
        let module_ast = parse_source(source)?;
        self.emit_module(&module_ast)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bin_ops() {
        let mut module: Module = Module::default();
        let mut emitter = Emitter::new(&mut module);
        emitter
            .emit(
                "(defn calc : f32
                    [a : f32 b : i32]
                        (* 10 (/ (+ a (- b 1)) 2)))",
            )
            .unwrap();
        assert_eq!(module.exports, []);
        assert_eq!(module.signatures.len(), 1);
        assert_eq!(
            *module.signatures.keys().next().unwrap(),
            Signature {
                sig_type: SignatureType::Func,
                params: vec![PrimitiveType::F32, PrimitiveType::I32],
                results: vec![PrimitiveType::F32]
            }
        );
        let module_functions = module.functions.borrow_mut();
        assert_eq!(
            module_functions["calc"].1,
            Function {
                arg_types: vec![Rc::new(Type::F32), Rc::new(Type::I32)],
                result_type: Rc::new(Type::F32),
                signature_index: 0,
                body: vec![
                    OpCode::LocalDeclCount(0),
                    OpCode::I32Const(10),
                    OpCode::F32ConvertI32S,
                    OpCode::LocalGet(0),
                    OpCode::LocalGet(1),
                    OpCode::I32Const(1),
                    OpCode::I32Sub,
                    OpCode::F32ConvertI32S,
                    OpCode::F32Add,
                    OpCode::I32Const(2),
                    OpCode::F32ConvertI32S,
                    OpCode::F32Div,
                    OpCode::F32Mul,
                    OpCode::End
                ]
            }
        )
    }
    #[test]
    fn test_unary_ops() {
        let mut module: Module = Module::default();
        let mut emitter = Emitter::new(&mut module);
        emitter
            .emit(
                "(defn neg_f32: f32
                            [n: f32]
                                (- n))
                        (defn neg_i32: i32
                            [n: i32]
                                (- n))",
            )
            .unwrap();
        assert_eq!(
            module.functions.borrow_mut()["neg_f32"].1.body,
            vec![
                OpCode::LocalDeclCount(0),
                OpCode::LocalGet(0),
                OpCode::F32Neg,
                OpCode::End
            ]
        );
        let module_functions = module.functions.borrow_mut();
        assert_eq!(
            module_functions["neg_i32"].1.body,
            vec![
                OpCode::LocalDeclCount(0),
                OpCode::I32Const(0),
                OpCode::LocalGet(0),
                OpCode::I32Sub,
                OpCode::End
            ]
        )
    }
    #[test]
    fn test_export() {
        let mut module: Module = Module::default();
        let mut emitter = Emitter::new(&mut module);
        emitter
            .emit(
                "(export defn addTwo : i32
                    [a : i32 b: i32]
                        (+ a b))",
            )
            .unwrap();
        assert_eq!(
            module.exports,
            vec![Export {
                export_type: ExportKind::Func,
                func_index: 0,
                name: "addTwo".to_string()
            }]
        )
    }
    #[test]
    fn test_function_call() {
        let mut module = Module::default();
        let mut emitter = Emitter::new(&mut module);
        emitter.emit("
            (defn addTwo: i32 [a: i32, b: i32] (+ a b) )
            (export defn main []
                (addTwo 10 20))
        ").unwrap();
        let module_functions = module.functions.borrow_mut();
        assert_eq!(
            module_functions["main"].1.body,
            vec![
                OpCode::LocalDeclCount(0),
                OpCode::I32Const(10),
                OpCode::I32Const(20),
                OpCode::Call(0),
                OpCode::Drop,
                OpCode::End
            ]
        )
    }
}