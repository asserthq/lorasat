use std::{future::pending, io::Write};

use host_sil::config::{CMD_PORT, MTU, SAT_ADDR};
use sat_core::protocol::{GroundCommand, GroundCommandResponse};
use sat_csp::RdpClient;
use tokio::io::{AsyncBufReadExt, BufReader};

const MESSAGE_BUFFER_SIZE: usize = 64;

enum Input {
    Command(GroundCommand),
    Help,
    Quit,
    Empty,
}

pub async fn run(client: RdpClient<'_, MTU>) {
    println!("ground: connecting command session to satellite...");
    let mut connection = match client.connect(SAT_ADDR.get(), CMD_PORT).await {
        Ok(connection) => {
            println!("ground: command session connected");
            connection
        }
        Err(error) => {
            eprintln!("ground: command session failed: {error}");
            eprintln!("ground: continuing in receive-only mode");
            return pending().await;
        }
    };

    print_help();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();

    loop {
        print!("gs> ");
        let _ = std::io::stdout().flush();

        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => {
                println!("\nground: stdin closed; continuing in receive-only mode");
                return pending().await;
            }
            Err(error) => {
                eprintln!("\nground: stdin error: {error}; continuing in receive-only mode");
                return pending().await;
            }
        };

        let command = match parse(&line) {
            Ok(Input::Command(command)) => command,
            Ok(Input::Help) => {
                print_help();
                continue;
            }
            Ok(Input::Quit) => return,
            Ok(Input::Empty) => continue,
            Err(error) => {
                eprintln!("ground: {error}; type 'help' for available commands");
                continue;
            }
        };

        let mut request_buffer = [0; MESSAGE_BUFFER_SIZE];
        let request = match command.encode(&mut request_buffer) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("ground: failed to encode {command:?}: {error}");
                continue;
            }
        };

        println!("ground: sending {command:?}");
        if let Err(error) = connection.send(request).await {
            eprintln!("ground: command delivery failed: {error}");
            continue;
        }

        let response = connection.recv().await;
        match GroundCommandResponse::decode(response.payload()) {
            Ok(response) => println!("ground: response: {response:?}"),
            Err(error) => eprintln!(
                "ground: invalid response from {}:{}: {error}; raw={:02x?}",
                response.source(),
                response.source_port(),
                response.payload(),
            ),
        }
    }
}

fn parse(line: &str) -> Result<Input, String> {
    let mut words = line.split_whitespace();
    let Some(command) = words.next() else {
        return Ok(Input::Empty);
    };

    let input = match command {
        "ping" => Input::Command(GroundCommand::Ping),
        "telemetry" => Input::Command(GroundCommand::RequestSatTelemetry),
        "client-data" => Input::Command(GroundCommand::RequestClientData),
        "beacon" => Input::Command(GroundCommand::ChangeBeaconInterval(parse_argument(
            &mut words,
            "beacon <seconds>",
        )?)),
        "time" => Input::Command(GroundCommand::SetTime(parse_argument(
            &mut words,
            "time <unix-seconds>",
        )?)),
        "help" => Input::Help,
        "quit" | "exit" => Input::Quit,
        other => return Err(format!("unknown command {other:?}")),
    };

    if words.next().is_some() {
        return Err(format!("unexpected extra arguments for {command:?}"));
    }
    Ok(input)
}

fn parse_argument<T>(
    words: &mut impl Iterator<Item = impl AsRef<str>>,
    usage: &str,
) -> Result<T, String>
where
    T: core::str::FromStr,
{
    words
        .next()
        .ok_or_else(|| format!("usage: {usage}"))?
        .as_ref()
        .parse()
        .map_err(|_| format!("usage: {usage}"))
}

fn print_help() {
    println!(
        "commands:\n  ping\n  telemetry\n  client-data\n  beacon <seconds>\n  time <unix-seconds>\n  help\n  quit"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_commands() {
        assert!(matches!(
            parse("ping"),
            Ok(Input::Command(GroundCommand::Ping))
        ));
        assert!(matches!(
            parse("telemetry"),
            Ok(Input::Command(GroundCommand::RequestSatTelemetry))
        ));
        assert!(matches!(
            parse("beacon 15"),
            Ok(Input::Command(GroundCommand::ChangeBeaconInterval(15)))
        ));
        assert!(matches!(
            parse("time 1234"),
            Ok(Input::Command(GroundCommand::SetTime(1234)))
        ));
    }

    #[test]
    fn rejects_invalid_arguments() {
        assert!(parse("beacon").is_err());
        assert!(parse("beacon nope").is_err());
        assert!(parse("ping extra").is_err());
        assert!(parse("unknown").is_err());
    }
}
