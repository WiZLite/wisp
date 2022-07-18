use crate::{
    emitter::{Emitter, Export, ExportKind, Function, Module, OpCode, Signature},
    encoder::{encode_leb128, encode_s_leb128, encode_string},
};
use anyhow::Result;
use std::io::{BufWriter, Write};

pub enum SectionCode {
    Type = 0x01,
    Function = 0x03,
    Export = 0x07,
    Code = 0x0a,
}

fn write_signature<W: Write>(writer: &mut W, signature: &Signature) -> Result<()> {
    // signature type
    writer.write(&[signature.sig_type as u8])?;
    // num params
    writer.write(&[signature.params.len() as u8])?;
    // params
    writer.write(
        &signature
            .params
            .iter()
            .map(|p| *p as u8)
            .collect::<Vec<_>>()[..],
    )?;
    // num results
    writer.write(&[signature.results.len() as u8])?;
    // results
    writer.write(
        &signature
            .results
            .iter()
            .map(|r| *r as u8)
            .collect::<Vec<_>>()[..],
    )?;
    Ok(())
}

fn write_export(writer: &mut impl Write, export: &Export) -> Result<()> {
    encode_string(writer, &export.name)?;
    writer.write(&[match export.export_type {
        ExportKind::Func => 0x00,
    }])?;
    encode_leb128(writer, export.func_index)?;
    Ok(())
}

fn write_function_body(writer: &mut impl Write, func: &Function) -> Result<()> {
    for opcode in &func.body {
        match opcode {
            OpCode::LocalDeclCount(count) => {
                encode_leb128(writer, *count)?;
            }
            OpCode::F32Const(n) => {
                writer.write(&[0x43])?;
                writer.write(&n.to_le_bytes())?;
            }
            OpCode::I32Const(n) => {
                writer.write(&[0x41])?;
                encode_s_leb128(writer, *n)?;
            }
            OpCode::LocalGet(n) => {
                writer.write(&[0x20])?;
                encode_leb128(writer, *n)?;
            }
            _ => {
                writer.write(&[match opcode {
                    OpCode::LocalDeclCount(_)
                    | OpCode::F32Const(_)
                    | OpCode::I32Const(_)
                    | OpCode::LocalGet(_) => unreachable!(),
                    OpCode::End => 0x0B,
                    OpCode::I32Add => 0x6A,
                    OpCode::I32Sub => 0x6B,
                    OpCode::I32Mul => 0x6C,
                    OpCode::I32Div => 0x6D,
                    OpCode::F32Neg => 0x8C,
                    OpCode::F32Add => 0x92,
                    OpCode::F32Sub => 0x93,
                    OpCode::F32Mul => 0x94,
                    OpCode::F32Div => 0x95,
                    OpCode::F32ConvertI32S => 0xB2,
                }])?;
            }
        }
    }
    Ok(())
}

fn write_type_section(writer: &mut impl Write, module: &Module) -> Result<()> {
    writer.write(&[SectionCode::Type as u8])?;
    let mut type_section = Vec::new();
    // write num type signatures
    encode_leb128(&mut type_section, module.signatures.len() as u64)?;
    let mut signatures_with_index = module.signatures.iter().collect::<Vec<_>>();
    signatures_with_index.sort_by(|a, b| a.1.partial_cmp(b.1).unwrap());
    for signature in signatures_with_index.iter().map(|x| x.0) {
        write_signature(&mut type_section, signature)?;
    }
    dbg!(&type_section.len());
    encode_leb128(writer, type_section.len() as u64)?; // section size
    writer.write(&type_section[..])?;
    writer.flush()?;
    Ok(())
}

fn write_function_section(writer: &mut impl Write, module: &Module) -> Result<()> {
    writer.write(&[SectionCode::Function as u8])?;
    let mut func_section = Vec::new();
    let num_functions = module.functions.len();
    encode_leb128(&mut func_section, num_functions as u64)?;
    for func in &module.functions {
        encode_leb128(&mut func_section, func.signature_index)?;
    }
    let section_size = func_section.len();
    encode_leb128(writer, section_size as u64)?;
    writer.write(&func_section)?;
    Ok(())
}

fn write_export_section(writer: &mut impl Write, module: &Module) -> Result<()> {
    writer.write(&[SectionCode::Export as u8])?;
    let mut export_section = Vec::new();
    let num_exports = module.exports.len();
    encode_leb128(&mut export_section, num_exports as u64)?;
    for export in &module.exports {
        write_export(&mut export_section, &export)?;
    }
    let section_size = export_section.len();
    encode_leb128(writer, section_size as u64)?;
    writer.write(&export_section)?;
    Ok(())
}

fn write_code_section(writer: &mut impl Write, module: &Module) -> Result<()> {
    writer.write(&[SectionCode::Code as u8])?;
    let mut code_section = Vec::new();
    let num_functions = module.functions.len();
    encode_leb128(&mut code_section, num_functions as u64)?;
    for func in &module.functions {
        let mut func_body_bytes = Vec::new();
        write_function_body(&mut func_body_bytes, func)?;
        // write func body size
        encode_leb128(&mut code_section, func_body_bytes.len() as u64)?;
        code_section.write(&func_body_bytes)?;
    }
    let section_size = code_section.len();
    encode_leb128(writer, section_size as u64)?;
    writer.write(&code_section)?;
    Ok(())
}

pub fn compile_into_wasm<W: Write>(writer: &mut BufWriter<W>, source: &str) -> Result<()> {
    let mut module = Module::default();
    let mut emitter = Emitter::new(&mut module);

    emitter.emit(source).unwrap();

    writer.write(&[0x00, 0x61, 0x73, 0x6d])?; // WASM magic number
    writer.write(&[0x01, 0x00, 0x00, 0x00])?; // WASM binary version

    write_type_section(writer, &module)?;
    writer.flush()?;

    write_function_section(writer, &module)?;
    writer.flush()?;

    write_export_section(writer, &module)?;
    writer.flush()?;

    write_code_section(writer, &module)?;
    writer.flush()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bin_ops() {
        let mut buf = Vec::<u8>::new();
        unsafe {
            let mut writer = BufWriter::new((&mut buf as *mut Vec<_>).as_mut().unwrap());
            compile_into_wasm(
                &mut writer,
                "(defn calc : f32
                (a : f32 b : i32)
                  (* 10 (/ (+ a (- b 1)) 2))",
            ).unwrap();
        }
        assert_eq!(
            buf,
            vec![
                0x00, 0x61, 0x73, 0x6d, // wasm header
                0x01, 0x00, 0x00, 0x00, // wasm binary version
                0x01, // type section
                0x07, // section size
                0x01, // num types
                0x60, // type func
                0x02, // num params
                0x6F, // f32
                0x7F, // i32
                0x01, // num results
                0x6F, // f32
                0x03, // function section
                0x02, // section size
                0x01, // num funcs
                0x00, // signature index
                0x07, // export section
                0x01, // section size,
                0x00, // num exports
                0x0A, // code section
                0x15, // section size
                0x01, // num functions
                0x13, // func body size
                0x00, // local decl count
                0x41, 0x0A, // i32 const 10
                0xB2, // f32_convert_i32_s
                0x20, 0x00, // local.get 0
                0x20, 0x01, // local.get 1,
                0x41, 0x01, // i32.const 1
                0x6B, // i32.sub
                0xB2, // f32_convert_i32_s
                0x92, // f32.add
                0x41, 0x02, // i32.const 2
                0xB2, // f32_convert_i32_s
                0x95, // f32.div
                0x94, // f32.mul
                0x0B, // END
            ]
        );
    }
}
