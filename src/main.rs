mod headers;

fn main() {
    if let Ok((_rest, (headers, _complete))) = headers::headers(b"Hello: world\r\n\r\n") {
        let h = &headers[0];
        println!(
            "{:?} ({}): {:?} ({})",
            h.name.name, h.name.flags, h.value.value, h.value.flags
        );
    }
}
