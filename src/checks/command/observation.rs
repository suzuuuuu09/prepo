#[derive(Clone, Copy, Debug)]
pub(super) struct Observation {
    pub(super) matched: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum ObserveError {
    LimitExceeded,
    ReadFailed,
}

struct StreamingMatcher<'a> {
    limit: usize,
    total: usize,
    pattern: Option<&'a [u8]>,
    matched: bool,
    tail: Vec<u8>,
}

impl<'a> StreamingMatcher<'a> {
    fn new(limit: usize, pattern: Option<&'a [u8]>) -> Self {
        Self {
            limit,
            total: 0,
            pattern,
            matched: false,
            tail: Vec::new(),
        }
    }

    fn observe(&mut self, chunk: &[u8]) -> Result<(), ObserveError> {
        self.total = self.total.saturating_add(chunk.len());
        if self.total > self.limit {
            return Err(ObserveError::LimitExceeded);
        }
        let Some(pattern) = self.pattern else {
            return Ok(());
        };
        let mut candidate = Vec::with_capacity(self.tail.len() + chunk.len());
        candidate.extend_from_slice(&self.tail);
        candidate.extend_from_slice(chunk);
        self.matched |= candidate
            .windows(pattern.len())
            .any(|window| window == pattern);
        let tail_length = pattern.len().saturating_sub(1).min(candidate.len());
        self.tail = candidate[candidate.len() - tail_length..].to_vec();
        Ok(())
    }
}

pub(super) async fn observe_stream<R>(
    mut reader: Option<R>,
    pattern: Option<&[u8]>,
) -> Result<Observation, ObserveError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;

    let Some(reader) = reader.as_mut() else {
        return Ok(Observation { matched: false });
    };
    let mut matcher = StreamingMatcher::new(8 * 1024 * 1024, pattern);
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(|_| ObserveError::ReadFailed)?;
        if count == 0 {
            return Ok(Observation {
                matched: matcher.matched,
            });
        }
        matcher.observe(&buffer[..count])?;
    }
}

#[cfg(test)]
mod tests {
    use super::StreamingMatcher;

    #[test]
    fn matches_across_chunks() {
        let mut matcher = StreamingMatcher::new(32, Some(b"abcd"));
        matcher.observe(b"ab").unwrap();
        matcher.observe(b"cd").unwrap();
        assert!(matcher.matched);
    }

    #[test]
    fn rejects_the_first_byte_over_the_limit() {
        let mut matcher = StreamingMatcher::new(3, None);
        assert!(matcher.observe(b"123").is_ok());
        assert!(matches!(
            matcher.observe(b"4"),
            Err(super::ObserveError::LimitExceeded)
        ));
    }
}
