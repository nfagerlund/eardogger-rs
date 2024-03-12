#![cfg(test)]

use super::tokens::*;
use super::users::*;
use super::Db;

#[tokio::test]
async fn cascading_delete() {
    let db = Db::new_test_db().await;
    let users = db.users();
    let tokens = db.tokens();
    let user1 = users.create("user1", "pass1", None).await.unwrap();
    let (token1, _token1text) = tokens
        .create(user1.id, TokenScope::WriteDogears, Some("token1"))
        .await
        .unwrap();
    let (_token2, _token1text) = tokens
        .create(user1.id, TokenScope::ManageDogears, Some("token2"))
        .await
        .unwrap();
    // ensure token delete works like expected
    tokens.destroy(token1.id, user1.id).await.unwrap();
    let (token_list, _meta) = tokens.list(user1.id, 1, 50).await.unwrap();
    assert_eq!(token_list.len(), 1);
    // ensure user delete works like expected
    users.destroy(user1.id).await.unwrap();
    assert!(users
        .authenticate("user1", "pass1")
        .await
        .unwrap()
        .is_none());
    assert_eq!(tokens.list(user1.id, 1, 50).await.unwrap().0.len(), 0);
}
