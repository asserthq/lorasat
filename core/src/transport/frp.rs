use crate::data_link::DataLinkLayer;
use crate::transport::TransportLayer;

struct FrpTransport<L: DataLinkLayer> {
    link: L,
}

impl<L: DataLinkLayer> TransportLayer for FrpTransport<L> {
    type Error = super::frp_error::FrpError;

    async fn try_send_message(&mut self, data: &[u8]) -> Result<(), Error> {
        // 1. Создать SenderSession (разбить data на чанки)
        let mut session = SenderSession::new(data, self.config.chunk_size, new_session_id());

        // 2. Отправить все чанки через link.send_frame()
        while let Some(packet) = session.next_packet() {
            let data = Data {
                kind: DataKind::SatelliteData,
                src_addr: self.src_addr,
                dest_addr: self.dest_addr,
                flags: 0,
                data: heapless::Vec::from_slice(&packet)?,
            };
            self.link.send_frame(Frame::DataFrame(data)).await?;
        }

        // 3. Ждать FrpCheck от получателя
        let check = self.wait_for_check(session.session_id).await?;

        // 4. Если есть пропущенные чанки — дослать
        session.process_check(&check);
        while let Some(packet) = session.next_packet() {
            // ... досылаем пропущенное
        }

        Ok(())
    }

    async fn try_recv_message(&mut self, buf: &mut [u8]) -> Result<&mut [u8], Error> {
        // 1. Ждать FrpStart
        let start = self.wait_for_start().await?;

        // 2. Создать ReceiverSession
        let mut session = ReceiverSession::new(self.config.max_message_size);
        session.handle_start(&start);

        // 3. Принимать FrpData, на каждый слать FrpAck
        while !session.is_complete() {
            let (data_frame, is_data) = self.wait_for_data_or_timeout().await?;
            if is_data {
                session.handle_data(&data_frame);
                self.send_ack(data_frame.session_id, data_frame.chunk_index)
                    .await?;
            }
        }

        // 4. Отправить FrpCheck, ждать ретрансмитов если есть дыры
        let check = session.generate_check();
        self.send_check(&check).await?;

        // 5. Скопировать результат в buf
        let msg = session.assembled_message();
        let len = msg.len();
        buf[..len].copy_from_slice(msg);
        Ok(&mut buf[..len])
    }
}
