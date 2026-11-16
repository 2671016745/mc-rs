//! 端到端登录握手测试：本地 mock 服务器（未压缩 Set Compression + 压缩 Login Success）。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn vi(v: i32) -> Vec<u8> {
    let mut x = v as u32;
    let mut out = Vec::new();
    loop {
        if (x & !0x7F) == 0 {
            out.push(x as u8);
            return out;
        }
        out.push(((x & 0x7F) | 0x80) as u8);
        x >>= 7;
    }
}

fn vi_parse(d: &[u8]) -> (i32, usize) {
    let mut n = 0u32;
    let mut sh = 0;
    let mut o = 0;
    loop {
        let b = d[o];
        o += 1;
        n |= ((b & 0x7f) as u32) << sh;
        if b & 0x80 == 0 {
            return (n as i32, o);
        }
        sh += 7;
    }
}

fn rd_varint(s: &mut impl Read) -> i32 {
    let mut n = 0u32;
    let mut sh = 0;
    loop {
        let mut b = [0u8; 1];
        s.read_exact(&mut b).unwrap();
        n |= ((b[0] & 0x7f) as u32) << sh;
        if b[0] & 0x80 == 0 {
            return n as i32;
        }
        sh += 7;
    }
}

fn rd_frame(s: &mut impl Read) -> (i32, Vec<u8>) {
    let len = rd_varint(s);
    let mut buf = vec![0u8; len as usize];
    s.read_exact(&mut buf).unwrap();
    let (id, n) = vi_parse(&buf);
    (id, buf[n..].to_vec())
}

fn rd_frame_c(s: &mut impl Read) -> (i32, Vec<u8>) {
    let len = rd_varint(s);
    let mut buf = vec![0u8; len as usize];
    s.read_exact(&mut buf).unwrap();
    let (dl, n) = vi_parse(&buf);
    let payload = &buf[n..];
    let data = if dl > 0 {
        use flate2::read::ZlibDecoder;
        let mut out = Vec::new();
        ZlibDecoder::new(payload).read_to_end(&mut out).unwrap();
        out
    } else {
        payload.to_vec()
    };
    let (id, m) = vi_parse(&data);
    (id, data[m..].to_vec())
}

#[test]
fn offline_login_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        assert_eq!(rd_frame(&mut s).0, 0, "handshake");

        let (id, body) = rd_frame(&mut s);
        assert_eq!(id, 0, "login start");
        let nl = body[0] as usize;
        let name = String::from_utf8(body[1..1 + nl].to_vec()).unwrap();
        let uuid = body[1 + nl..1 + nl + 16].to_vec();

        // Set Compression（未压缩格式帧）
        let mut fr = vi(3); // 包体 = VarInt(id=3) + VarInt(256) = 3 字节
        fr.extend_from_slice(&vi(3));
        fr.extend_from_slice(&vi(256));
        s.write_all(&fr).unwrap();

        // Login Success（压缩格式帧，dataLen=0）
        let mut data = vi(2);
        data.extend_from_slice(&uuid);
        data.push(name.len() as u8);
        data.extend_from_slice(name.as_bytes());
        data.push(0);
        let mut fr = vi((data.len() + 1) as i32);
        fr.extend_from_slice(&vi(0));
        fr.extend_from_slice(&data);
        s.write_all(&fr).unwrap();

        assert_eq!(rd_frame_c(&mut s).0, 3, "login acknowledged");
        name
    });

    let res = mc_rs::net::login_offline("127.0.0.1", port, "TestPlayer").unwrap();
    assert_eq!(server.join().unwrap(), "TestPlayer");
    assert_eq!(res.name, "TestPlayer");
    assert_eq!(
        res.uuid.to_string(),
        "bb77495a-a740-3169-a238-69654c8bd2c1",
        "离线 UUID 应与 Java 一致"
    );
}
