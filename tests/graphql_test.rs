use std::panic;

use serde_json::json;

#[macro_use]
extern crate lazy_static;

mod common;

use common::graphql::*;

#[test]
fn test_generic() {
    let mut tester = init_app();

    tester.login_root();
    // Create site
    let site_id = tester.submit(query(r#"mutation {
        addSite(data: {}) { id }
    }"#))["id"].to_i64();

    // Change name
    let res = tester.submit(
        query(r#"mutation updateSite($id: Int!) {
            updateSite(id: $id, data: { name: "testmuse" }) { id, name }
        }"#).add_variable("id", site_id)
    );
    assert_eq!(res, json!({ "id": site_id, "name": "testmuse" }));

    // Add sensor
    let res = tester.submit(query(r#"mutation addSensor($id: Int!) {
        addSensor(siteId: $id, data: { name: "testsensor" }) { id, siteId }
    }"#).add_variable("id", site_id));

    let sensor_id = res["id"].to_i64();
    assert_eq!(res["siteId"], site_id);

    // Add sensor location
    let res = tester.submit(query(r#"mutation addSensorData($id: Int!) {
        updateSensor(id: $id, data: { locX: 1234, locY: 5678 }) { locX, locY }
    }"#).add_variable("id", sensor_id));
    assert_eq!(res, json!({"locX": 1234, "locY": 5678}));

    // TODO: Test Map image data

    // Add channel
    let res = tester.submit(query(r#"mutation addChannel($sensorId: Int!) {
        addChannel(sensorId: $sensorId, data: { name: "pioppo", measureUnit: "nonno" }) { id, name, measureUnit }
    }"#).add_variable("sensorId", sensor_id));
    let channel_id = res["id"].to_i64();
    assert_eq!(res, json!({"id": channel_id, "name": "pioppo", "measureUnit": "nonno"}));

    // Cleanup
    tester.submit(query(r#"mutation deleteSite($id: Int!) {
        deleteSite(id: $id)
    }"#).add_variable("id", site_id));
}

#[test]
fn test_permission_view() {
    let mut tester = init_app();
    let mut paolo_tester = tester.clone();

    tester.login_root();

    let site_ids: Vec<i64> = (0..3).map(|_| {
        tester.submit(query(r#"mutation {
            addSite(data: {}) { id }
        }"#))["id"].to_i64()
    }).collect();

    let (user_id, user_name) = tester.create_random_user("123");

    tester.submit(query(r#"mutation giveAccess($userId: Int!, $siteIds: [Int!]!) {
        giveUserAccess(userId: $userId, siteIds: $siteIds)
    }"#).add_variable("userId", user_id).add_variable("siteIds", &site_ids[0..=1]));

    paolo_tester.login(&user_name, "123");
    let res = paolo_tester.submit(query(r#"query { sites { id } }"#));
    assert_eq!(res, json!([
        {"id": site_ids[0]},
        {"id": site_ids[1]}
    ]));

    let res = paolo_tester.submit_raw(query(r#"query getSingleSite($id: Int!) {
        site (id: $id) { id }
    }"#).add_variable("id", site_ids[2]));

    res.expect_service_error("NOT_FOUND");

    // Cleanup
    for id in site_ids {
        tester.submit(query(r#"mutation deleteSite($id: Int!) {
            deleteSite(id: $id)
        }"#).add_variable("id", id));
    }
    tester.submit(query(r#"mutation deleteUser($id: Int!) {
        deleteUser(id: $id)
    }"#).add_variable("id", user_id));
}

#[test]
fn test_delete_cascade() {
    let mut tester = init_app();

    tester.login_root();

    // Create site
    let site_id = tester.submit(query(r#"mutation {
        addSite(data: {}) { id }
    }"#))["id"].to_i64();

    // Add sensor
    let sensor_id = tester.submit(query(r#"mutation addSensor($id: Int!) {
        addSensor(siteId: $id, data: { name: "testsensor" }) { id, siteId }
    }"#).add_variable("id", site_id))["id"].to_i64();

    // Add channel
    let channel_id = tester.submit(query(r#"mutation addChannel($sensorId: Int!) {
        addChannel(sensorId: $sensorId, data: { name: "pioppo", measureUnit: "nonno" }) { id, name, measureUnit }
    }"#).add_variable("sensorId", sensor_id))["id"].to_i64();

    // Delete site
    tester.submit(query(r#"mutation deleteSite($id: Int!) {
        deleteSite(id: $id)
    }"#).add_variable("id", site_id));

    // We shouldn't be able to find neither the sensor nor the channel
    let res = tester.submit_raw(query(r#"query findChannel($id: Int!) {
        channel(id: $id) { sensorId }
    }"#).add_variable("id", channel_id));
    res.expect_service_error("NOT_FOUND");

    let res = tester.submit_raw(query(r#"query findSensor($id: Int!) {
        sensor(id: $id) { siteId }
    }"#).add_variable("id", sensor_id));
    res.expect_service_error("NOT_FOUND");
}

// TODO: test readings
// TODO: test contacter

#[test]
fn test_user_password_misc() {
    let mut tester = init_app();
    let mut user_tester = tester.clone();

    tester.login_root();

    // Create site
    let site_id = tester.submit(query(r#"mutation {
        addSite(data: {}) { id }
    }"#))["id"].to_i64();

    // Create users
    let (user1_id, _user1_name) = tester.create_random_user("password11");
    let (user2_id, _user2_name) = tester.create_random_user("password12");

    // An admin should be able to change any user's data (both username and password)
    let user1_name = create_random_username();
    tester.submit(
        query(r#"mutation changeUserQuery1($userId: Int!, $newName: String!, $newPass: String!) {
            updateUser(id: $userId, data: {
                username: $newName,
                password: $newPass,
            }) { id }
        }"#)
            .add_variable("userId", user1_id)
            .add_variable("newName", user1_name.clone())
            .add_variable("newPass", "password12")
    );

    // An user should not be able to change another user's details
    user_tester.login(&user1_name, "password12");
    user_tester.submit_raw(
        query(r#"mutation changeUserNameMutation($userId: Int!, $newName: String!) {
            updateUser(id: $userId, data: { username: $newName }) { id }
        }"#)
            .add_variable("userId", user2_id)
            .add_variable("newName", create_random_username())
    ).expect_service_error("UNAUTHORIZED");

    // Check token invalidation on password change
    tester.submit(
        query(r#"mutation changeUserQuery2($userId: Int!, $newPass: String!) {
            updateUser(id: $userId, data: {
                password: $newPass,
            }) { id }
        }"#)
        .add_variable("userId", user1_id)
        .add_variable("newPass", "password13")
    );
    user_tester.submit_raw(query("query { userMe { id } }")).expect_service_error("LOGIN_REQUIRED");

    // Cleanup
    tester.submit(
        query(r#"mutation cleanupUserPasswordMisc($siteId: Int!, $user1Id: Int!, $user2Id: Int!) {
            a1: deleteSite(id: $siteId)
            a2: deleteUser(id: $user1Id)
            a3: deleteUser(id: $user2Id)
        }"#)
            .add_variable("siteId", site_id)
            .add_variable("user1Id", user1_id)
            .add_variable("user2Id", user2_id)
    );
}

// TODO: test alarm controller
// TODO: test image resizing
