table! {
    channel (id) {
        id -> Int4,
        sensor_id -> Int4,
        id_cnr -> Nullable<Varchar>,
        name -> Nullable<Varchar>,
        measure_unit -> Nullable<Varchar>,
        range_min -> Nullable<Numeric>,
        range_max -> Nullable<Numeric>,
    }
}

table! {
    fcm_user_contact (registration_id) {
        registration_id -> Varchar,
        user_id -> Int4,
    }
}

table! {
    sensor (id) {
        id -> Int4,
        site_id -> Int4,
        id_cnr -> Nullable<Varchar>,
        name -> Nullable<Varchar>,
        loc_x -> Nullable<Int4>,
        loc_y -> Nullable<Int4>,
        enabled -> Bool,
        status -> Varchar,
    }
}

table! {
    site (id) {
        id -> Int4,
        name -> Nullable<Varchar>,
        id_cnr -> Nullable<Varchar>,
    }
}

table! {
    user_access (user_id, site_id) {
        user_id -> Int4,
        site_id -> Int4,
    }
}

table! {
    user_account (id) {
        id -> Int4,
        username -> Varchar,
        password_hash -> Varchar,
        last_password_change -> Timestamp,
        permission -> Bpchar,
    }
}

joinable!(channel -> sensor (sensor_id));
joinable!(fcm_user_contact -> user_account (user_id));
joinable!(sensor -> site (site_id));
joinable!(user_access -> site (site_id));
joinable!(user_access -> user_account (user_id));

allow_tables_to_appear_in_same_query!(
    channel,
    fcm_user_contact,
    sensor,
    site,
    user_access,
    user_account,
);
