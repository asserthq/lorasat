use core::mem::size_of;

struct Header {
    src: u32,
    dest: u32,
} // 8

struct Packet {
    header: Header,
    payload: [u8; 32],
} // 40

fn main() {
    let hdr = size_of::<Header>();
    let pkt = size_of::<Packet>();

    println!("header: {hdr}");
    println!("packet: {pkt}");
}
