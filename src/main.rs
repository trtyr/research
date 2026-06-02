use anyhow::Result;
use clap::Parser;
use research::{Cli, run};
use serde::Serialize;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let json = cli.json;
    match run(cli) {
        Ok(data) if json => print_json(JsonResult {
            ok: true,
            data: Some(data),
            error: None,
        })?,
        Ok(data) => println!("{}", serde_json::to_string_pretty(&data)?),
        Err(error) if json => {
            print_json::<serde_json::Value>(JsonResult {
                ok: false,
                data: None,
                error: Some(format!("{error:#}")),
            })?;
            std::process::exit(1);
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

#[derive(Serialize)]
struct JsonResult<T: Serialize> {
    ok: bool,
    data: Option<T>,
    error: Option<String>,
}

fn print_json<T: Serialize>(result: JsonResult<T>) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
