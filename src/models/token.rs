mod scopes;

use chrono::NaiveDateTime;
use diesel::prelude::*;

pub use self::scopes::{CrateScope, EndpointScope};
use crate::models::User;
use crate::schema::api_tokens;
use crate::util::errors::{AppResult, InsecurelyGeneratedTokenRevoked};
use crate::util::rfc3339;
use crate::util::token::{SecureToken, SecureTokenKind};

/// The model representing a row in the `api_tokens` database table.
#[derive(Clone, Debug, PartialEq, Eq, Identifiable, Queryable, Associations, Serialize)]
#[diesel(belongs_to(User))]
pub struct ApiToken {
    pub id: i32,
    #[serde(skip)]
    pub user_id: i32,
    #[serde(skip)]
    token: SecureToken,
    pub name: String,
    #[serde(with = "rfc3339")]
    pub created_at: NaiveDateTime,
    #[serde(with = "rfc3339::option")]
    pub last_used_at: Option<NaiveDateTime>,
    #[serde(skip)]
    pub revoked: bool,
    /// `None` or a list of crate scope patterns (see RFC #2947)
    #[serde(skip)]
    pub crate_scopes: Option<Vec<CrateScope>>,
    /// A list of endpoint scopes or `None` for the `legacy` endpoint scope (see RFC #2947)
    #[serde(skip)]
    pub endpoint_scopes: Option<Vec<EndpointScope>>,
}

impl ApiToken {
    /// Generates a new named API token for a user
    pub fn insert(conn: &mut PgConnection, user_id: i32, name: &str) -> AppResult<CreatedApiToken> {
        Self::insert_with_scopes(conn, user_id, name, None, None)
    }

    pub fn insert_with_scopes(
        conn: &mut PgConnection,
        user_id: i32,
        name: &str,
        crate_scopes: Option<Vec<CrateScope>>,
        endpoint_scopes: Option<Vec<EndpointScope>>,
    ) -> AppResult<CreatedApiToken> {
        let token = SecureToken::generate(SecureTokenKind::Api);

        let model: ApiToken = diesel::insert_into(api_tokens::table)
            .values((
                api_tokens::user_id.eq(user_id),
                api_tokens::name.eq(name),
                api_tokens::token.eq(&*token),
                api_tokens::crate_scopes.eq(crate_scopes),
                api_tokens::endpoint_scopes.eq(endpoint_scopes),
            ))
            .get_result(conn)?;

        Ok(CreatedApiToken {
            plaintext: token.plaintext().into(),
            model,
        })
    }

    pub fn find_by_api_token(conn: &mut PgConnection, token_: &str) -> AppResult<ApiToken> {
        use crate::schema::api_tokens::dsl::*;
        use diesel::{dsl::now, update};

        let token_ = SecureToken::parse(SecureTokenKind::Api, token_)
            .ok_or_else(InsecurelyGeneratedTokenRevoked::boxed)?;

        let tokens = api_tokens
            .filter(revoked.eq(false))
            .filter(token.eq(&token_));

        // If the database is in read only mode, we can't update last_used_at.
        // Try updating in a new transaction, if that fails, fall back to reading
        conn.transaction(|conn| {
            update(tokens)
                .set(last_used_at.eq(now.nullable()))
                .get_result(conn)
        })
        .or_else(|_| tokens.first(conn))
        .map_err(Into::into)
    }
}

pub struct CreatedApiToken {
    pub model: ApiToken,
    pub plaintext: String,
}

// Use a custom implementation of Debug to hide the plaintext token.
impl std::fmt::Debug for CreatedApiToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreatedApiToken")
            .field("model", &self.model)
            .field("plaintext", &"(sensitive)")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::EncodableApiTokenWithToken;
    use chrono::NaiveDate;

    #[test]
    fn api_token_serializes_to_rfc3339() {
        let tok = ApiToken {
            id: 12345,
            user_id: 23456,
            token: SecureToken::generate(SecureTokenKind::Api).into_inner(),
            revoked: false,
            name: "".to_string(),
            created_at: NaiveDate::from_ymd_opt(2017, 1, 6)
                .unwrap()
                .and_hms_opt(14, 23, 11)
                .unwrap(),
            last_used_at: Some(
                NaiveDate::from_ymd_opt(2017, 1, 6)
                    .unwrap()
                    .and_hms_opt(14, 23, 12),
            )
            .unwrap(),
            crate_scopes: None,
            endpoint_scopes: None,
        };
        let json = serde_json::to_string(&tok).unwrap();
        assert_some!(json
            .as_str()
            .find(r#""created_at":"2017-01-06T14:23:11+00:00""#));
        assert_some!(json
            .as_str()
            .find(r#""last_used_at":"2017-01-06T14:23:12+00:00""#));
    }

    #[test]
    fn encodeable_api_token_with_token_serializes_to_rfc3339() {
        let tok = EncodableApiTokenWithToken {
            id: 12345,
            name: "".to_string(),
            token: "".to_string(),
            revoked: false,
            created_at: NaiveDate::from_ymd_opt(2017, 1, 6)
                .unwrap()
                .and_hms_opt(14, 23, 11)
                .unwrap(),
            last_used_at: Some(
                NaiveDate::from_ymd_opt(2017, 1, 6)
                    .unwrap()
                    .and_hms_opt(14, 23, 12),
            )
            .unwrap(),
        };
        let json = serde_json::to_string(&tok).unwrap();
        assert_some!(json
            .as_str()
            .find(r#""created_at":"2017-01-06T14:23:11+00:00""#));
        assert_some!(json
            .as_str()
            .find(r#""last_used_at":"2017-01-06T14:23:12+00:00""#));
    }
}
