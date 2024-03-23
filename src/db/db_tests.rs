#![cfg(test)]
//! This module collects a bunch of tests that slam the actual database, to
//! verify expected application-level behaviors. Traditionally that's a bit
//! of a slog, but while running the migrations wastes a bit of time, at least
//! sqlite makes it real easy to do the in-memory DB thing.
//!
//! These tests aren't quite as clutch now as they were in eardogger 1 --
//! sqlx's type-checked query macros are honestly pretty game changing, and
//! if you can get it to compile it's generally gonna work as expected.
//! Still, porting the tests is a good way to verify that my port is accurate.

use sqlx::query;
use time::{Duration, OffsetDateTime};

use crate::util::ListMeta;

use super::tokens::TokenScope;
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
    let (wrong_token, _) = tokens
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
    // 404
    assert!(wrong_destroy.expect("no err").is_none());
    let right_destroy = tokens.destroy(right_token.id, right_user.id).await;
    assert!(right_destroy.expect("no err").is_some());
    let huh_destroy = tokens.destroy(right_token.id, right_user.id).await;
    // can't re-delete
    assert!(huh_destroy.expect("no err").is_none());
    // can't authenticate a destroyed token
    let gone_auth = tokens
        .authenticate(&right_cleartext)
        .await
        .expect("shouldn't error");
    assert!(gone_auth.is_none());
}

#[tokio::test]
async fn user_password_auth() {
    let db = Db::new_test_db().await;
    let users = db.users();

    // basic peep
    let user = users
        .create("test_peep", "aoeuhtns", Some("nf@example.com"))
        .await
        .expect("usr create err");
    assert_eq!(user.username, "test_peep");
    assert_eq!(user.email.as_deref(), Some("nf@example.com"));
    // No blank usernames
    assert!(users.create("", "aoeua", None).await.is_err());
    // No blank passwords (this is a change from eardogger 1, where that just disabled login)
    assert!(users.create("", "", None).await.is_err());
    // No spaces in username
    assert!(users.create("space cadet", "aoeu", None).await.is_err());
    // Space in pw ok tho
    assert!(users
        .create("spacecadet", " im in space", None)
        .await
        .is_ok());
    assert!(users
        .authenticate("spacecadet", " im in space")
        .await
        .expect("shouldn't error")
        .is_some());
    // Wrong password gets you nothin
    assert!(users
        .authenticate("spacecadet", "")
        .await
        .expect("shouldn't error")
        .is_none());
    // Authenticate trims space on username
    assert!(users
        .authenticate(" spacecadet ", " im in space")
        .await
        .expect("shouldn't error")
        .is_some());
    // Authenticate doesn't trim space on passwords
    assert!(users
        .authenticate("spacecadet", " im in space ")
        .await
        .expect("shouldn't error")
        .is_none());
    // Nonexistent user gets Ok(None), just like wrong pw.
    assert!(users
        .authenticate("blasecadet", "spaaaaace")
        .await
        .expect("shouldn't error")
        .is_none());

    // EDIT PASSWORD
    // news must match
    assert!(users
        .change_password(user.username.as_str(), "aoeuhtns", "snthueoa", "snthsnth")
        .await
        .is_err());
    // ok, so:
    assert!(users
        .change_password(user.username.as_str(), "aoeuhtns", "snthueoa", "snthueoa")
        .await
        .is_ok());
    // new pw works
    assert!(users
        .authenticate(user.username.as_str(), "snthueoa")
        .await
        .expect("no err")
        .is_some());

    // EDIT EMAIL
    // blank same as none.
    // Difference from eardogger 1: set_email used to return user, now it returns Result<()>.
    for &(email, cleaned) in &[
        (None, None),
        (Some("newpeep@example.com"), Some("newpeep@example.com")),
        (Some(""), None),
    ] {
        assert!(users.set_email(user.username.as_str(), email).await.is_ok());
        assert_eq!(
            users
                .by_name(user.username.as_str())
                .await
                .expect("no err")
                .expect("some")
                .email
                .as_deref(),
            cleaned
        );
    }
}

#[tokio::test]
async fn dogears() {
    let db = Db::new_test_db().await;
    let dogears = db.dogears();
    let user = db.users().create("peep", "boop", None).await.unwrap();
    let wrong_user = db.users().create("wrong", "bop", None).await.unwrap();

    // New user, empty list.
    let (list, meta) = dogears.list(user.id, 1, 50).await.expect("no err");
    assert!(list.is_empty());
    assert_eq!(
        meta,
        ListMeta {
            count: 0,
            page: 1,
            size: 50
        }
    );

    // CREATE:
    let dogear = dogears
        .create(
            user.id,
            "example.com/comic/",
            "https://example.com/comic/240",
            Some("Example Comic"),
        )
        .await
        .expect("no err");
    // exercise prefix normalization while I'm here
    let second = dogears
        .create(
            user.id,
            "http://www.example.com/story/",
            "https://example.com/story/2",
            None,
        )
        .await
        .expect("no err");
    assert_eq!(second.prefix.as_str(), "example.com/story/");
    // Difference from eardogger 1: used to be able to omit `current` at
    // db level, but not anymore.
    let _third = dogears
        .create(
            user.id,
            "example.com/extras/",
            "http://example.com/extras/turnarounds",
            None,
        )
        .await
        .expect("no err");
    // LIST: now there's three
    let (list, meta) = dogears.list(user.id, 1, 50).await.expect("no err");
    assert_eq!(list.len(), 3);
    assert_eq!(meta.count, 3);
    // Unrelated user: empty list still
    let (list, _) = dogears.list(wrong_user.id, 1, 50).await.expect("no err");
    assert_eq!(list.len(), 0);

    // CURRENTLY
    let earlier = &dogear.current;
    // Difference from eardogger 1: Used to be able to check currently on a
    // fragment of a URL, like "example.com/comic/2".
    for &url in &[
        "https://example.com/comic/1",
        // misc schemes, non-signifying subdomains
        "http://m.example.com/comic/4",
        "https://www.example.com/comic/   ", // trailing whatever
    ] {
        let currently = dogears
            .current_for_site(user.id, url)
            .await
            .expect("no err")
            .expect("some");
        assert_eq!(&currently, earlier);
    }
    // Non-matching URL:
    assert!(dogears
        .current_for_site(user.id, "https://example.com/commie")
        .await
        .expect("no err")
        .is_none());

    // UPDATE
    // Difference from eardogger 1: used to strip whitespace from input URLs, but
    // not anymore.
    for &url in &[
        "https://example.com/comic/241",
        "https://m.example.com/comic/242",
        "http://www.example.com/comic/243",
    ] {
        let updated = dogears
            .update(user.id, url)
            .await
            .expect("no err")
            .expect("some");
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].current.as_str(), url);
    }
    // Non-matching url
    assert!(dogears
        .current_for_site(user.id, "https://example.com/com/not-dogeared")
        .await
        .expect("no err")
        .is_none());
    // Can't double-create an existing dogear.
    // Difference from eardogger 1: this used to silently upsert on conflict.
    // But, asking for all the extra info beyond current is pointless if you're
    // throwing it away anyway, right? And I never gave an interface to explicitly
    // *edit* a dogear. All told, it feels like a 2019 mis-design, so I'm undoing it.
    assert!(dogears
        .create(
            user.id,
            dogear.prefix.as_str(),
            "http://example.com/comic/249",
            None,
        )
        .await
        .is_err());
    let (list, _) = dogears.list(user.id, 1, 50).await.expect("no err");
    // Unchanged:
    assert_eq!(list.len(), 3);

    // DESTROY
    // safety switch: user_id needs to match
    assert!(dogears.destroy(second.id, wrong_user.id).await.is_err());
    assert!(dogears.destroy(second.id, user.id).await.is_ok());
    // list shrinks
    let (list, _) = dogears.list(user.id, 1, 50).await.expect("no err");
    assert_eq!(list.len(), 2);
}
