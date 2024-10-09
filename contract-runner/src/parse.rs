use starknet::core::utils::get_selector_from_name;
use std::path::PathBuf;

pub fn parse(filename: &PathBuf) {
    use std::env;

    let current_dir = env::current_dir().expect("Failed to get current directory");
    println!("Current directory: {}", current_dir.display());

    use std::path::Path;
    use std::process::Command;

    // Get the directory of the file
    let dir = filename.parent().expect("Failed to get parent directory");

    // Change to the directory containing the file
    std::env::set_current_dir(dir).expect("Failed to change directory");

    // Run 'scarb expand' command
    let output = Command::new("scarb")
        .arg("expand")
        .output()
        .expect("Failed to execute scarb expand command");

    if !output.status.success() {
        eprintln!(
            "Error running scarb expand: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        // Read the expanded Cairo file
        let expanded_file_path = Path::new("../target/dev/cairo_ex.expanded.cairo");
        match std::fs::read_to_string(expanded_file_path) {
            Ok(expanded_content) => {
                let updated = prepare(expanded_content);
                // Save the updated content to checker.cairo
                let checker_file_path = Path::new("checker.cairo");

                match std::fs::write(checker_file_path, &updated) {
                    Ok(_) => println!("Updated content saved to checker.cairo"),
                    Err(e) => eprintln!("Failed to save updated content: {}", e),
                }
            }
            Err(e) => {
                eprintln!("Failed to read expanded Cairo file: {}", e);
            }
        }
    }

    // Change back to the original directory
    std::env::set_current_dir(current_dir).expect("Failed to change back to original directory");
}

fn prepare(expanded_content: String) -> String {
    let lines = expanded_content.lines().collect::<Vec<&str>>();

    let mut entrypoints = Vec::new();

    for l in lines.iter() {
        if l.contains("fn ") {
            if let Some(start_index) = l.find("fn ") {
                if let Some(end_index) = l[start_index..].find('(') {
                    let function_name = l[start_index + 3..start_index + end_index].trim();
                    entrypoints.push(function_name.to_string());
                }
            }
        } else if l.contains("}") {
            break;
        }
    }

    let entrypoints = entrypoints
        .into_iter()
        .map(|n| {
            (
                get_selector_from_name(&n)
                    .expect("invalid selector name")
                    .to_string(),
                n,
            )
        })
        .collect::<Vec<_>>();

    dbg!(&entrypoints);

    let dispatch_calls = entrypoints
        .into_iter()
        .map(|(n, s)| {
            let call = format!(
                "cairo_ex::HelloStarknet::__wrapper__HelloStarknetImpl__{s}(call.calldata)"
            );
            format!("if call.selector == {n} {{{call}}} else ",)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut updated_lines = Vec::new();

    let mut skip_lines = 0;

    for line in lines {
        if skip_lines > 0 {
            updated_lines.push("//".to_string() + line);
            skip_lines -= 1;
            continue;
        } else if line.contains("#[starknet::interface]")
            || line.contains("#[abi(embed_v0)]")
            || line.contains("#[event]")
            || line.contains("starknet::Event")
        {
            updated_lines.push("//".to_string() + line);
        } else if line.contains("core::gas::withdraw_gas_all(core::gas::get_builtin_costs())") {
            updated_lines.pop();
            updated_lines.push("//".to_string() + line);
            skip_lines = 1;
        } else if line.contains("mod") || line.contains("fn __wrapper") {
            updated_lines.push("pub ".to_string() + line);
        } else if line.contains("System") || line.contains("core::gas") {
            updated_lines.push("//".to_string() + line);
        } else if line.contains("impl ContractStateEventEmitter") {
            updated_lines.push("//".to_string() + line);
            skip_lines = u64::MAX;
        } else {
            updated_lines.push(line.to_string());
        }
    }

    if skip_lines > 0 {
        updated_lines.push("}}".to_string());
    }

    let header = r#"
        #[derive(Serde, Drop)]
        struct Calls {
            selector: felt252,
            calldata: Span<felt252>,
        }

        #[derive(Serde, Drop)]
        struct Args {
            calls: Array<Calls>,
        }

        fn main(input: Array<felt252>) -> Array<felt252> {
            let mut input = input.span();
            let mut args = Serde::<Args>::deserialize(ref input).unwrap();

            let mut r = array![];

            loop {
                let call = match args.calls.pop_front() {
                    Option::Some(call) => call,
                    Option::None => { break; },
                };

                let ret = "#;

    let footer = r#"{
                    panic(array!['Invalid selector']);
                    array![].span()
                };

                r.append_span(ret);
            };

            r
        }
    "#;

    let main = format!("{header}{dispatch_calls}{footer}");

    updated_lines.push(main.to_string());

    updated_lines.join("\n")
}

#[cfg(test)]
mod tests {
    use cairo_lang_compiler::{
        compile_prepared_db, db::RootDatabase, project::setup_project, CompilerConfig,
    };

    use super::*;

    #[test]
    fn test_parse() {
        let filename = PathBuf::from("../../contract-binary-interpreter/cairo/src/contract.cairo");

        parse(&filename.parent().unwrap().join("lib.cairo"));

        let compiler_config = CompilerConfig {
            replace_ids: true,
            ..CompilerConfig::default()
        };

        let mut db = RootDatabase::builder()
            .detect_corelib()
            .skip_auto_withdraw_gas()
            .build()
            .unwrap();

        let main_crate_ids = setup_project(&mut db, &filename).unwrap();
        let _sierra_program_with_dbg =
            compile_prepared_db(&db, main_crate_ids, compiler_config).unwrap();
    }
}
