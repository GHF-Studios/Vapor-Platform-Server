/// Pending temporary SSH requests expire after three days, whether they are
/// inspected or granted through the web service or the local operator tool.
pub const PENDING_TTL_SECONDS: i64 = 3 * 24 * 60 * 60;
