//! Key counts for the database tree, without selecting or scanning each database.

use crate::{ServerDb, error::Error, probe::info_fields};
use redis::cmd;
use zedis_core::features::{ReplyClass, classify_reply};

/// One keyspace section per primary covers every logical database. If the
/// server withholds it, only the selected database's DBSIZE is known.
pub async fn database_key_counts(at: &ServerDb) -> Result<Vec<Option<u64>>, Error> {
    let client = at.client().await?;
    let databases = client.databases();
    if databases > 1 && !client.info_unavailable() {
        let mut info = cmd("INFO");
        info.arg("keyspace");
        if let Ok((_, replies)) = client.query_async_masters::<String>(vec![info]).await
            && let Some(counts) = keyspace_counts(&replies, databases)
        {
            return Ok(counts.into_iter().map(Some).collect());
        }
    }
    let mut counts = vec![None; databases];
    if let Some(current) = counts.get_mut(at.db()) {
        *current = match client.dbsize().await {
            Ok(count) => Some(count),
            Err(Error::Redis { source })
                if matches!(
                    classify_reply(source.code(), source.detail().unwrap_or_default()),
                    ReplyClass::Denied | ReplyClass::Missing
                ) =>
            {
                None
            }
            Err(error) => return Err(error),
        };
    }
    Ok(counts)
}

fn keyspace_counts(replies: &[String], databases: usize) -> Option<Vec<u64>> {
    if replies.is_empty() {
        return None;
    }
    let mut counts = vec![0u64; databases];
    for reply in replies {
        let mut has_section = reply.lines().any(|line| line.trim().eq_ignore_ascii_case("# Keyspace"));
        for (name, fields) in info_fields(reply) {
            let Some(db) = name.strip_prefix("db").and_then(|db| db.parse::<usize>().ok()) else {
                continue;
            };
            let keys = fields.split(',').find_map(|f| f.strip_prefix("keys="))?.parse().ok()?;
            if let Some(total) = counts.get_mut(db) {
                *total = total.checked_add(keys)?;
            }
            has_section = true;
        }
        if !has_section {
            return None;
        }
    }
    Some(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_discovery_counts_every_db_and_distinguishes_empty_from_unavailable() {
        let counts = |replies: &[&str]| keyspace_counts(&replies.iter().map(|s| s.to_string()).collect::<Vec<_>>(), 4);
        assert_eq!(
            counts(&[
                "# Keyspace\r\ndb0:keys=318,expires=2\r\ndb2:keys=208,expires=0",
                "# Keyspace\ndb2:keys=3"
            ]),
            Some(vec![318, 0, 211, 0])
        );
        assert_eq!(counts(&["# Keyspace\r\n"]), Some(vec![0; 4]));
        assert_eq!(counts(&["# Keyspace\n", "# Server\n"]), None);
        assert_eq!(counts(&["# Keyspace\ndb1:keys=?"]), None);
    }
}
