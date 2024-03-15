#![cfg(test)]
//! This module collects a bunch of tests that slam the actual database, to
//! verify expected application-level behaviors. Traditionally that's a fairly
//! wasteful activity, but I think sqlite should make this a bit lighter weight
//! than I'm used to it being.

use sqlx::query;
use time::{Duration, OffsetDateTime};

use super::dogears::*;
use super::sessions::*;
use super::tokens::*;
use super::users::*;
use super::Db;

#[tokio::test]
async fn cascading_delete() {
    let db = Db::new_test_db().await;
    let users = db.users();
    let tokens = db.tokens();
    let sessions = db.sessions();
    let dogears = db.dogears();

    // create user
    let user1 = users.create("user1", "pass1", None).await.unwrap();
    // create token, check existence
    let _ = tokens
        .create(user1.id, TokenScope::WriteDogears, Some("token1"))
        .await
        .unwrap();
    let (token_list, _meta) = tokens.list(user1.id, 1, 50).await.unwrap();
    assert_eq!(token_list.len(), 1);
    // create session, check existence
    let session1 = sessions.create(user1.id).await.unwrap();
    assert!(sessions.authenticate(&session1.id).await.unwrap().is_some());
    // create dogear, check existence
    let _ = dogears
        .create(
            user1.id,
            "example.com/comic",
            "http://www.example.com/comic/32",
            Some("Legends of the RFC 2606"),
        )
        .await
        .unwrap();
    let (dogear_list, _meta) = dogears.list(user1.id, 1, 50).await.unwrap();
    assert_eq!(dogear_list.len(), 1);

    // FINALLY: delete user and verify cascade
    users.destroy(user1.id).await.unwrap();
    assert!(users
        .authenticate("user1", "pass1")
        .await
        .unwrap()
        .is_none());
    // no tokens
    assert!(tokens.list(user1.id, 1, 50).await.unwrap().0.is_empty());
    // no sessions
    assert!(sessions.authenticate(&session1.id).await.unwrap().is_none());
    // no dogears
    assert!(dogears.list(user1.id, 1, 50).await.unwrap().0.is_empty());
}

#[tokio::test]
async fn session_lifetime_modifier() {
    // HARDCODED ASSUMPTION: sessions::SESSION_LIFETIME_MODIFIER is +90 days.
    let db = Db::new_test_db().await;

    let session_user = db
        .users()
        .create("session_guy", "none-shall-pass", None)
        .await
        .expect("failed user creation");
    let session = db
        .sessions()
        .create(session_user.id)
        .await
        .expect("failed to get session");
    // make sure time crate + sqlx is doing what we expect.
    // to wit: all sqlite date/time values are stored as UTC and retrieved as UTC.
    assert!(session.expires.offset().is_utc());
    let now = OffsetDateTime::now_utc();
    let delta = session.expires - now;
    // touching time in a test is always hairy, but we'll just give it some slop:
    assert!(delta > Duration::days(89));
    assert!(delta < Duration::days(91));
    // ...as a treat.

    // Now, check that authenticate does the expected thing. First, manually dink
    // w/ the db:
    let sessid = session.id.as_str();
    let too_soon = query!(
        r#"
            UPDATE sessions
            SET expires = datetime('now', '+1 day')
            WHERE id = ?
            RETURNING expires;
        "#,
        sessid,
    )
    .fetch_one(&db.pool)
    .await
    .unwrap()
    .expires;
    // Session is now about to expire:
    assert!(too_soon - now < Duration::days(2));
    let (new_session, _) = db
        .sessions()
        .authenticate(sessid)
        .await
        .expect("sess auth error")
        .expect("sess auth None");
    let new_delta = new_session.expires - now;
    // expiry got reset:
    assert!(new_delta > Duration::days(89));
    assert!(new_delta < Duration::days(91));
}

#[tokio::test]
async fn token_create_auth_destroy() {
    let db = Db::new_test_db().await;
    let users = db.users();
    let tokens = db.tokens();

    // CREATE
    let right_user = users
        .create("rightTokenCreate", "password123", None)
        .await
        .expect("user create err");
    let wrong_user = users
        .create("wrongTokenCreate", "password456", None)
        .await
        .expect("user create err");
    let (right_token, right_cleartext) = tokens
        .create(right_user.id, TokenScope::WriteDogears, Some("comment"))
        .await
        .expect("token create err");
    let (wrong_token, wrong_cleartext) = tokens
        .create(wrong_user.id, TokenScope::WriteDogears, Some("nocomment"))
        .await
        .expect("token create err");
    // IDs increment
    assert_ne!(right_user.id, wrong_user.id);
    assert_ne!(right_token.id, wrong_token.id);
    // AUTH
    let (auth_token, auth_user) = tokens
        .authenticate(&right_cleartext)
        .await
        .expect("token auth err")
        .expect("token auth none");
    // got right token and user back
    assert_eq!(auth_user.id, right_user.id);
    assert_eq!(auth_token.id, right_token.id);
    // DESTROY
    let wrong_destroy = tokens.destroy(right_token.id, wrong_user.id).await;
    assert!(wrong_destroy.is_err());
    let right_destroy = tokens.destroy(right_token.id, right_user.id).await;
    assert!(right_destroy.is_ok());
    let huh_destroy = tokens.destroy(right_token.id, right_user.id).await;
    // can't re-delete
    assert!(huh_destroy.is_err());
    // can't authenticate a destroyed token
    let gone_auth = tokens
        .authenticate(&right_cleartext)
        .await
        .expect("shouldn't error");
    assert!(gone_auth.is_none());
}
