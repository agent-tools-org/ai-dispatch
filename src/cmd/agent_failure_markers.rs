// Lenient marker-only row decoding for advisory failure reports.
// Counts corrupt records while keeping valid marker rows available.
use super::*;
type Marker = (DateTime<Local>, String, String, String);

pub(super) fn markers(
    store: &Store,
    agent: Option<&str>,
    cutoff: DateTime<Local>,
) -> Result<(Vec<Marker>, usize)> {
    let conn = store.db();
    let mut query = conn.prepare(
        "SELECT task_id, timestamp, metadata FROM events
        WHERE event_type = 'milestone' AND CASE WHEN json_valid(metadata)
        THEN json_type(metadata, '$.unclassified_failure') = 'true'
        ELSE metadata LIKE '%unclassified_failure%' END",
    )?;
    let rows = query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut markers = Vec::new();
    let mut skipped = 0;
    for row in rows {
        let marker = row.ok().and_then(decode);
        let Some((time, id, name, excerpt)) = marker else {
            skipped += 1;
            continue;
        };
        if time >= cutoff && agent.is_none_or(|filter| filter == name) {
            markers.push((time, id, name, excerpt));
        }
    }
    Ok((markers, skipped))
}

fn decode((id, time, metadata): (String, String, String)) -> Option<Marker> {
    let time = DateTime::parse_from_rfc3339(&time)
        .ok()?
        .with_timezone(&Local);
    let data: serde_json::Value = serde_json::from_str(&metadata).ok()?;
    Some((
        time,
        id,
        data["agent"].as_str()?.to_string(),
        data["provider_excerpt"].as_str()?.to_string(),
    ))
}
