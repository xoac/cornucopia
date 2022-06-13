use std::{
    borrow::Cow,
    fmt::Display,
    process::{Command, ExitCode},
};

use clap::Parser;
use cornucopia::{container, CodegenSettings};
use owo_colors::OwoColorize;

/// Start cornucopia test runner
#[derive(Parser, Debug)]
#[clap(version)]
struct Args {
    /// Format test descriptors and update error msg
    #[clap(short, long)]
    apply: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct TestSuite<'a> {
    #[serde(borrow)]
    test: Vec<Test<'a>>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct Test<'a> {
    name: &'a str,
    query: Option<&'a str>,
    migration: Option<&'a str>,
    query_name: Option<&'a str>,
    migration_name: Option<&'a str>,
    error: Cow<'a, str>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    if test(args.apply) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn display<T, E: Display>(result: Result<T, E>) -> Result<T, E> {
    if let Err(err) = &result {
        eprintln!("{}", err);
    }
    result
}

// Run test, return true if all test are successful
fn test(apply: bool) -> bool {
    // Start by removing previous container if it was left open
    container::cleanup(false).ok();
    container::setup(false).unwrap();
    let successful = std::panic::catch_unwind(|| {
        let mut client = cornucopia::conn::cornucopia_conn().unwrap();
        display(run_errors_test(&mut client, apply)).unwrap()
            && display(run_codegen_test(&mut client)).unwrap()
            && display(run_examples_test(&mut client)).unwrap()
    });
    // Format all to prevent CLI errors
    Command::new("cargo").args(["fmt", "--all"]).output().ok();
    container::cleanup(false).unwrap();
    successful.unwrap()
}

// Reset the current database
fn reset_db(client: &mut postgres::Client) -> Result<(), postgres::Error> {
    client.batch_execute("DROP SCHEMA public CASCADE;CREATE SCHEMA public;")
}

// Run errors test, return true if all test are successful
fn run_errors_test(
    client: &mut postgres::Client,
    apply: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut successful = true;

    let got_msg = if apply {
        "Apply:".bright_black()
    } else {
        "Got:".bright_black()
    };
    let expected_msg = if apply {
        "Previous:".bright_black()
    } else {
        "Expected:".bright_black()
    };

    let original_pwd = std::env::current_dir().unwrap();
    for file in std::fs::read_dir("fixtures")? {
        let file = file?;
        let name = file.file_name().to_string_lossy().to_string();
        let content = std::fs::read_to_string(file.path())?;
        let mut suite: TestSuite = toml::from_str(&content)?;

        println!("{} {}", "[error]".magenta(), name.magenta());
        for test in &mut suite.test {
            // Generate file tree path
            let temp_dir = tempfile::tempdir()?;

            // Reset db
            reset_db(client)?;

            // We need to change current dir for error path to always be the same
            std::env::set_current_dir(&temp_dir)?;

            // Generate migrations files
            std::fs::create_dir("migrations")?;
            if let Some(migration) = test.migration {
                let name = test.migration_name.unwrap_or("1653210840_first.sql");
                std::fs::write(&format!("migrations/{name}"), migration)?;
            }

            // generate queries files
            std::fs::create_dir("queries")?;
            if let Some(query) = test.query {
                let name = test.query_name.unwrap_or("module_1.sql");
                std::fs::write(&format!("queries/{name}"), query)?;
            }

            // Run codegen
            let result: Result<(), cornucopia::Error> = (|| {
                cornucopia::run_migrations(client, "migrations")?;
                cornucopia::generate_live(
                    client,
                    "queries",
                    None,
                    CodegenSettings {
                        is_async: false,
                        derive_ser: false,
                    },
                )?;
                Ok(())
            })();

            let err = result.err().map(|e| e.to_string()).unwrap_or_default();
            if err.trim() != test.error.trim() {
                successful = false;
                println!(
                    "{} {}\n{}\n{}\n{}\n{}",
                    test.name,
                    "ERR".red(),
                    got_msg,
                    err,
                    expected_msg,
                    test.error
                );
            } else {
                println!("{} {}", test.name, "OK".green());
            }
            if apply {
                test.error = Cow::Owned(err.trim().to_string())
            }
            std::env::set_current_dir(&original_pwd)?;
        }

        if apply {
            // Format test descriptor and update error message if needed
            let edited = toml::to_string_pretty(&suite)?;
            std::fs::write(file.path(), edited)?;
        }
    }
    Ok(successful)
}

// Run codegen test, return true if all test are successful
fn run_codegen_test(client: &mut postgres::Client) -> Result<bool, Box<dyn std::error::Error>> {
    let mut successful = true;
    let original_pwd = std::env::current_dir().unwrap();

    std::env::set_current_dir("../codegen_test")?;
    // Reset db
    reset_db(client)?;

    // Run codegen
    cornucopia::run_migrations(client, "migrations")?;
    cornucopia::generate_live(
        client,
        "queries",
        Some("src/cornucopia_async.rs"),
        CodegenSettings {
            is_async: true,
            derive_ser: true,
        },
    )?;
    cornucopia::generate_live(
        client,
        "queries",
        Some("src/cornucopia_sync.rs"),
        CodegenSettings {
            is_async: false,
            derive_ser: true,
        },
    )?;

    // Run test
    print!("{}", "[codegen]".magenta(),);
    let result = Command::new("cargo").arg("run").output()?;
    if !result.status.success() {
        successful = false;
        println!(
            " {}\n{}",
            "ERR".red(),
            String::from_utf8_lossy(&result.stderr)
                .as_ref()
                .bright_black()
        );
    } else {
        println!(" {}", "OK".green());
    }

    std::env::set_current_dir(&original_pwd)?;
    Ok(successful)
}

// Run example test, return true if all test are successful
fn run_examples_test(client: &mut postgres::Client) -> Result<bool, Box<dyn std::error::Error>> {
    let mut successful = true;
    let original_pwd = std::env::current_dir().unwrap();
    for file in std::fs::read_dir("../examples")? {
        let file = file?;
        let name = file.file_name().to_string_lossy().to_string();
        let path = file.path();

        print!("{} {}", "[example]".magenta(), name.magenta());

        std::env::set_current_dir(path)?;
        // Reset db
        reset_db(client)?;

        // Run codegen
        cornucopia::run_migrations(client, "migrations")?;
        cornucopia::generate_live(
            client,
            "queries",
            Some("src/cornucopia.rs"),
            CodegenSettings {
                is_async: !name.contains("sync"),
                derive_ser: false,
            },
        )?;

        // Run example
        let result = Command::new("cargo").arg("run").output()?;
        if !result.status.success() {
            successful = false;
            println!(
                " {}\n{}",
                "ERR".red(),
                String::from_utf8_lossy(&result.stderr)
                    .as_ref()
                    .bright_black()
            );
        } else {
            println!(" {}", "OK".green());
        }
        std::env::set_current_dir(&original_pwd)?;
    }
    Ok(successful)
}

#[cfg(test)]
mod test {
    use crate::test;

    #[test]
    fn run() {
        assert!(test(false))
    }
}