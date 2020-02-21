#[macro_use]
extern crate lazy_static;

use std::panic;

use serde_json::json;

use common::graphql::*;
use actix_web::test::TestRequest;
use actix_web::http::header;
use actix_http::http::StatusCode;


mod common;

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

    // Test bulk
    let res = paolo_tester.submit(query(r#"query getBulkSites($siteIds: [Int!]!) {
        sites(ids: $siteIds) { id }
    }"#).add_variable("siteIds", &site_ids[0..=1]));

    assert_eq_set(res, json!([
        {"id": site_ids[0]},
        {"id": site_ids[1]}
    ]));

    let res = paolo_tester.submit_raw(query(r#"query getBulkSites($siteIds: [Int!]!) {
        sites(ids: $siteIds) { id }
    }"#).add_variable("siteIds", &site_ids[0..=2]));
    res.expect_service_error("NOT_FOUND");

    // Also test for sensors-channels
    // Built tree:
    // 0 (visible)
    // |- 00
    // ||- 000
    // 1 (visible)
    // |- 10
    // ||- 100
    // ||- 101
    // |- 11
    // ||- 110
    // 2 (not visible)
    // |- 20
    // ||- 200
    // ||- 201

    let res = tester.submit_all(
        query(r#"mutation createSensorsTPW($siteA: Int!, $siteB: Int!, $siteC: Int!) {
            s00: addSensor(siteId: $siteA, data: {}) { id }
            s10: addSensor(siteId: $siteB, data: {}) { id }
            s11: addSensor(siteId: $siteB, data: {}) { id }
            s20: addSensor(siteId: $siteC, data: {}) { id }
        }"#)
            .add_variable("siteA", site_ids[0])
            .add_variable("siteB", site_ids[1])
            .add_variable("siteC", site_ids[2])
    );
    let s00 = res["s00"]["id"].to_i64();
    let s10 = res["s10"]["id"].to_i64();
    let s11 = res["s11"]["id"].to_i64();
    let s20 = res["s20"]["id"].to_i64();

    let res = tester.submit_all(
        query(r#"mutation createChannelsTPW($s00: Int!, $s10: Int!, $s11: Int!, $s20: Int!) {
            c000: addChannel(sensorId: $s00, data: {}) { id }
            c100: addChannel(sensorId: $s10, data: {}) { id }
            c101: addChannel(sensorId: $s10, data: {}) { id }
            c110: addChannel(sensorId: $s11, data: {}) { id }
            c200: addChannel(sensorId: $s20, data: {}) { id }
            c201: addChannel(sensorId: $s20, data: {}) { id }
        }"#)
            .add_variable("s00", s00)
            .add_variable("s10", s10)
            .add_variable("s11", s11)
            .add_variable("s20", s20)
    );
    let c000 = res["c000"]["id"].to_i64();
    let c100 = res["c100"]["id"].to_i64();
    let c101 = res["c101"]["id"].to_i64();
    let c110 = res["c110"]["id"].to_i64();
    let c200 = res["c200"]["id"].to_i64();
    let c201 = res["c201"]["id"].to_i64();

    // Ok, setup done. now to the fun part:
    // Test bulk channels from admin (everything visible)
    let res = tester.submit(query(r#"query getBulkSensors($sensorIds: [Int!]!) {
        sensors(ids: $sensorIds) { id }
    }"#).add_variable("sensorIds", vec![s00, s10, s11, s20]));
    assert_eq_set(res, json!([
        {"id": s00},
        {"id": s10},
        {"id": s11},
        {"id": s20},
    ]));

    // Test bulk from admin with non-existant sensor
    let res = tester.submit_raw(query(r#"query getBulkSensors($sensorIds: [Int!]!) {
        sensors(ids: $sensorIds) { id }
    }"#).add_variable("sensorIds", vec![s00, s20, s20 * 100 + 1]));
    res.expect_service_error("NOT_FOUND");

    // Test bulk from non-admin
    let res = paolo_tester.submit(query(r#"query getBulkSensors($sensorIds: [Int!]!) {
        sensors(ids: $sensorIds) { id }
    }"#).add_variable("sensorIds", vec![s00, s10, s11]));
    assert_eq_set(res, json!([
        {"id": s00},
        {"id": s10},
        {"id": s11},
    ]));

    // Test bulk from non-admin with invisible sensor (s20)
    let res = paolo_tester.submit_raw(query(r#"query getBulkSensors($sensorIds: [Int!]!) {
        sensors(ids: $sensorIds) { id }
    }"#).add_variable("sensorIds", vec![s00, s10, s20]));
    res.expect_service_error("NOT_FOUND");


    // Channels test bulk from non-admins
    let res = paolo_tester.submit(query(r#"query getBulkChannels($channelIds: [Int!]!) {
        channels(ids: $channelIds) { id }
    }"#).add_variable("channelIds", vec![c000, c100, c110]));
    assert_eq_set(res, json!([
        {"id": c000},
        {"id": c100},
        {"id": c110},
    ]));

    // Channels test bulk from non-admins with invisible channel
    let res = paolo_tester.submit_raw(query(r#"query getBulkChannels($channelIds: [Int!]!) {
        channels(ids: $channelIds) { id }
    }"#).add_variable("channelIds", vec![c000, c101, c110, c201]));
    res.expect_service_error("NOT_FOUND");

    // Same as last but from admin
    // Channels test bulk from non-admins
    let res = tester.submit(query(r#"query getBulkChannels($channelIds: [Int!]!) {
        channels(ids: $channelIds) { id }
    }"#).add_variable("channelIds", vec![c000, c101, c110, c200, c201]));
    assert_eq_set(res, json!([
        {"id": c000},
        {"id": c101},
        {"id": c110},
        {"id": c200},
        {"id": c201},
    ]));

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
    let res = user_tester.submit(query("query { userMe { id } }"));
    assert_eq!(res, json!(null));

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

#[test]
fn test_image_resize() {
    let mut tester = init_app();
    tester.login_root();

    // Create site
    let site_id = tester.submit(query(r#"mutation {
        addSite(data: {}) { id }
    }"#))["id"].to_i64();
    let site_map_uri = format!("/api/site_map/{}", site_id);

    let res = tester.submit_raw_req(
        TestRequest::post()
            .uri(&format!("{}?width={}&height={}", site_map_uri, 3840, 2160))
            .header(header::CONTENT_TYPE, "image/png")
            .set_payload("first png image")
    );
    assert_eq!(StatusCode::OK, res.0);

    // Create sensors
    let res = tester.submit_all(
        query(r#"mutation createSensorsTIR($siteId: Int!) {
            s1: addSensor(siteId: $siteId, data: { locX: 10,  locY: 20  }) { id }
            s2: addSensor(siteId: $siteId, data: { locX: 2,   locY: 3   }) { id }
            s3: addSensor(siteId: $siteId, data: { locX: 413, locY: 125 }) { id }
        }"#)
            .add_variable("siteId", site_id)
    );
    let s1 = res["s1"]["id"].to_i64();
    let s2 = res["s2"]["id"].to_i64();
    let s3 = res["s3"]["id"].to_i64();

    let res = tester.submit_raw_req(TestRequest::get().uri(&site_map_uri));
    assert_eq!(StatusCode::OK, res.0);
    assert_eq!("first png image", res.1);

    let res = tester.submit_raw_req(
        TestRequest::post()
            .uri(&format!("{}?width={}&height={}", site_map_uri, 7680, 4320))
            .header(header::CONTENT_TYPE, "image/png")
            .set_payload("second png image")
    );
    assert_eq!(StatusCode::OK, res.0);

    // Check that the images have been resized
    let res = tester.submit(
        query(r#"
        query querySitesTIR($siteId: Int!) {
            site(id: $siteId) {
                sensors {
                    id, locX, locY
                }
            }
        }"#).add_variable("siteId", site_id)
    );
    assert_eq_set(
        json!([
            { "id": s1, "locX": 20, "locY": 40 },
            { "id": s2, "locX": 4, "locY": 6 },
            { "id": s3, "locX": 826, "locY": 250 }
        ]),
        res["sensors"].clone()
    );

    // Cleanup
    tester.submit(query(r#"mutation deleteSite($id: Int!) {
        deleteSite(id: $id)
    }"#).add_variable("id", site_id));
}
