//! Fixtures shared by the server's integration tests: a seeded service
//! and raw HTTP/1.1 responses.

pub(crate) use nail_core::testing::TempService as Fixture;

/// A quiet service (no config watching, engines off, secrets in memory)
/// with the demo data.
pub(crate) fn service() -> Fixture {
    Fixture::seeded()
}

/// A parsed HTTP/1.1 response.
#[derive(Debug)]
pub(crate) struct Http {
    pub(crate) status: u16,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl Http {
    /// Parses a whole response as read from the connection (a chunked
    /// body is decoded).
    pub(crate) fn parse(raw: &[u8]) -> Self {
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("a response head");
        let head = String::from_utf8_lossy(&raw[..split]).into_owned();
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .unwrap()
            .split(' ')
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        let headers: Vec<(String, String)> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
            .collect();
        let mut body = raw[split + 4..].to_vec();
        if headers
            .iter()
            .any(|(n, v)| n == "transfer-encoding" && v.contains("chunked"))
        {
            body = decode_chunked(&body);
        }
        Self {
            status,
            headers,
            body,
        }
    }

    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

fn decode_chunked(mut data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(end) = data.windows(2).position(|w| w == b"\r\n") else {
            return out;
        };
        let size = usize::from_str_radix(
            std::str::from_utf8(&data[..end])
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .trim(),
            16,
        )
        .unwrap();
        if size == 0 {
            return out;
        }
        let start = end + 2;
        out.extend_from_slice(&data[start..start + size]);
        data = &data[start + size + 2..];
    }
}
