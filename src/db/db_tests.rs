#![cfg(test)]
//! This module collects a bunch of tests that slam the actual database, to
//! verify expected application-level behaviors. Traditionally that's a fairly
//! wasteful activity, but I think sqlite should make this a bit lighter weight
//! than I'm used to it being.

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
