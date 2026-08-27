//! Command-line entry point for the `CityGML` converter.

#[allow(dead_code)]
mod metadata;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Strict,
    Tolerant,
    Inspect,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Command {
    input: PathBuf,
    output: Option<PathBuf>,
    mode: Mode,
}

fn main() -> ExitCode {
    match parse_command(env::args().skip(1)) {
        Ok(command) => run(command),
        Err(message) => {
            eprintln!(
                "{message}\nusage: citymodel <convert|inspect> <input> [--output <directory>] [--strict|--tolerant]"
            );
            ExitCode::from(2)
        }
    }
}
fn parse_command(arguments: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut values = arguments.into_iter();
    let action = values.next().ok_or("missing command")?;
    let input = PathBuf::from(values.next().ok_or("missing input")?);
    let mut output = None;
    let mut mode = if action == "inspect" {
        Mode::Inspect
    } else if action == "convert" {
        Mode::Strict
    } else {
        return Err("unknown command".to_owned());
    };
    while let Some(argument) = values.next() {
        match argument.as_str() {
            "--output" => {
                output = Some(PathBuf::from(
                    values.next().ok_or("missing --output value")?,
                ));
            }
            "--strict" => mode = Mode::Strict,
            "--tolerant" => mode = Mode::Tolerant,
            _ => return Err(format!("unknown option: {argument}")),
        }
    }
    Ok(Command {
        input,
        output,
        mode,
    })
}
fn run(command: Command) -> ExitCode {
    if !command.input.exists() {
        eprintln!("input does not exist: {}", command.input.display());
        return ExitCode::from(1);
    }
    if command.mode == Mode::Inspect {
        println!(
            r#"{{"input":"{}","mode":"inspect","schemaVersion":"{}"}}"#,
            command.input.display(),
            citymodel_citygml::contract_schema_version()
        );
        return ExitCode::SUCCESS;
    }
    let Some(output) = command.output else {
        eprintln!("convert requires --output");
        return ExitCode::from(2);
    };
    match atomic_output_handoff(&output, |temporary| {
        fs::write(
            temporary.join("conversion.report.json"),
            format!(
                r#"{{"mode":"{:?}","input":"{}"}}"#,
                command.mode,
                command.input.display()
            ),
        )
    }) {
        Ok(()) => {
            println!("conversion output: {}", output.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("conversion failed: {error}");
            ExitCode::from(1)
        }
    }
}
fn atomic_output_handoff(
    output: &Path,
    write: impl FnOnce(&Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let temporary = output.with_extension("tmp-citymodel");
    if temporary.exists() {
        fs::remove_dir_all(&temporary)?;
    }
    fs::create_dir_all(&temporary)?;
    if let Err(error) = write(&temporary) {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }
    if output.exists() {
        fs::remove_dir_all(output)?;
    }
    fs::rename(temporary, output)
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    #[test]
    fn parses_inspect_and_tolerant_convert() {
        assert_eq!(
            parse_command(["inspect".into(), "x.gml".into()])
                .unwrap()
                .mode,
            Mode::Inspect
        );
        assert_eq!(
            parse_command([
                "convert".into(),
                "x.gml".into(),
                "--tolerant".into(),
                "--output".into(),
                "out".into()
            ])
            .unwrap()
            .mode,
            Mode::Tolerant
        );
    }
}
