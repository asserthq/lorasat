use defmt::{error, info};
use embedded_io_async::{Read, Write};
use sandbox_lib::shell::Shell;
use sat_core::layer::phy::PhyLayer;

use crate::commands;

pub async fn gs_task<U, P>(mut shell: Shell<U>, mut radio: P)
where
    U: Read + Write,
    P: PhyLayer,
    <P as PhyLayer>::Error: defmt::Format,
{
    info!("gs online");
    shell.inner_mut().write_all(b"gs> ").await.unwrap();

    let mut line = [0u8; 128];

    loop {
        let len = match shell.read_line(&mut line).await {
            Ok(len) => len,
            Err(e) => {
                error!("read_line failed: {:?}", e);
                continue;
            }
        };

        if len == 0 {
            shell.inner_mut().write_all(b"gs> ").await.unwrap();
            continue;
        }

        match commands::parse(&line[..len]) {
            Some(cmd) => {
                let mut buf = [0u8; 64];
                match postcard::to_slice(&cmd, &mut buf) {
                    Ok(ser) => match radio.send_bytes(ser).await {
                        Ok(()) => info!("tx {:?}", cmd),
                        Err(e) => error!("tx failed: {:?}", e),
                    },
                    Err(_) => error!("serialize failed"),
                }

                let mut reply_buf = [0u8; 255];
                match radio.recv_bytes(&mut reply_buf).await {
                    Ok(reply) => {
                        shell.inner_mut().write_all(b"sat: ").await.unwrap();
                        shell.inner_mut().write_all(reply).await.unwrap();
                        shell.inner_mut().write_all(b"\r\n").await.unwrap();
                        info!("rx reply: {}", reply);
                    }
                    Err(e) => error!("rx reply failed: {:?}", e),
                }
            }
            None => {
                shell
                    .inner_mut()
                    .write_all(b"unknown command\r\n")
                    .await
                    .unwrap();
            }
        }

        shell.inner_mut().write_all(b"gs> ").await.unwrap();
    }
}
