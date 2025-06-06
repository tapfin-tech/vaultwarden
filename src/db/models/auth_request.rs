use super::{DeviceId, OrganizationId, UserId};
use crate::{crypto::ct_eq, util::format_date};
use chrono::{NaiveDateTime, Utc};
use derive_more::{AsRef, Deref, Display, From};
use macros::UuidFromParam;
use serde_json::Value;

db_object! {
    #[derive(Debug, Identifiable, Queryable, Insertable, AsChangeset, Deserialize, Serialize)]
    #[diesel(table_name = auth_requests)]
    #[diesel(treat_none_as_null = true)]
    #[diesel(primary_key(uuid))]
    pub struct AuthRequest {
        pub uuid: AuthRequestId,
        pub user_uuid: UserId,
        pub organization_uuid: Option<OrganizationId>,

        pub request_device_identifier: DeviceId,
        pub device_type: i32,  // https://github.com/bitwarden/server/blob/9ebe16587175b1c0e9208f84397bb75d0d595510/src/Core/Enums/DeviceType.cs

        pub request_ip: String,
        pub response_device_id: Option<DeviceId>,

        pub access_code: String,
        pub public_key: String,

        pub enc_key: Option<String>,

        pub master_password_hash: Option<String>,
        pub approved: Option<bool>,
        pub creation_date: NaiveDateTime,
        pub response_date: Option<NaiveDateTime>,

        pub authentication_date: Option<NaiveDateTime>,
    }
}

impl AuthRequest {
    pub fn new(
        user_uuid: UserId,
        request_device_identifier: DeviceId,
        device_type: i32,
        request_ip: String,
        access_code: String,
        public_key: String,
    ) -> Self {
        let now = Utc::now().naive_utc();

        Self {
            uuid: AuthRequestId(crate::util::get_uuid()),
            user_uuid,
            organization_uuid: None,

            request_device_identifier,
            device_type,
            request_ip,
            response_device_id: None,
            access_code,
            public_key,
            enc_key: None,
            master_password_hash: None,
            approved: None,
            creation_date: now,
            response_date: None,
            authentication_date: None,
        }
    }

    pub fn to_json_for_pending_device(&self) -> Value {
        json!({
            "id": self.uuid,
            "creationDate": format_date(&self.creation_date),
        })
    }
}

use crate::db::DbConn;

use crate::api::EmptyResult;
use crate::error::MapResult;

impl AuthRequest {
    pub async fn save(&mut self, conn: &mut DbConn) -> EmptyResult {
        db_run! { conn:
            sqlite, mysql {
                match diesel::replace_into(auth_requests::table)
                    .values(AuthRequestDb::to_db(self))
                    .execute(conn)
                {
                    Ok(_) => Ok(()),
                    // Record already exists and causes a Foreign Key Violation because replace_into() wants to delete the record first.
                    Err(diesel::result::Error::DatabaseError(diesel::result::DatabaseErrorKind::ForeignKeyViolation, _)) => {
                        diesel::update(auth_requests::table)
                            .filter(auth_requests::uuid.eq(&self.uuid))
                            .set(AuthRequestDb::to_db(self))
                            .execute(conn)
                            .map_res("Error auth_request")
                    }
                    Err(e) => Err(e.into()),
                }.map_res("Error auth_request")
            }
            postgresql {
                let value = AuthRequestDb::to_db(self);
                diesel::insert_into(auth_requests::table)
                    .values(&value)
                    .on_conflict(auth_requests::uuid)
                    .do_update()
                    .set(&value)
                    .execute(conn)
                    .map_res("Error saving auth_request")
            }
        }
    }

    pub async fn find_by_uuid(uuid: &AuthRequestId, conn: &mut DbConn) -> Option<Self> {
        db_run! {conn: {
            auth_requests::table
                .filter(auth_requests::uuid.eq(uuid))
                .first::<AuthRequestDb>(conn)
                .ok()
                .from_db()
        }}
    }

    pub async fn find_by_uuid_and_user(uuid: &AuthRequestId, user_uuid: &UserId, conn: &mut DbConn) -> Option<Self> {
        db_run! {conn: {
            auth_requests::table
                .filter(auth_requests::uuid.eq(uuid))
                .filter(auth_requests::user_uuid.eq(user_uuid))
                .first::<AuthRequestDb>(conn)
                .ok()
                .from_db()
        }}
    }

    pub async fn find_by_user(user_uuid: &UserId, conn: &mut DbConn) -> Vec<Self> {
        db_run! {conn: {
            auth_requests::table
                .filter(auth_requests::user_uuid.eq(user_uuid))
                .load::<AuthRequestDb>(conn).expect("Error loading auth_requests").from_db()
        }}
    }

    pub async fn find_by_user_and_requested_device(
        user_uuid: &UserId,
        device_uuid: &DeviceId,
        conn: &mut DbConn,
    ) -> Option<Self> {
        db_run! {conn: {
            auth_requests::table
                .filter(auth_requests::user_uuid.eq(user_uuid))
                .filter(auth_requests::request_device_identifier.eq(device_uuid))
                .filter(auth_requests::approved.is_null())
                .order_by(auth_requests::creation_date.desc())
                .first::<AuthRequestDb>(conn).ok().from_db()
        }}
    }

    pub async fn find_created_before(dt: &NaiveDateTime, conn: &mut DbConn) -> Vec<Self> {
        db_run! {conn: {
            auth_requests::table
                .filter(auth_requests::creation_date.lt(dt))
                .load::<AuthRequestDb>(conn).expect("Error loading auth_requests").from_db()
        }}
    }

    pub async fn delete(&self, conn: &mut DbConn) -> EmptyResult {
        db_run! { conn: {
            diesel::delete(auth_requests::table.filter(auth_requests::uuid.eq(&self.uuid)))
                .execute(conn)
                .map_res("Error deleting auth request")
        }}
    }

    pub fn check_access_code(&self, access_code: &str) -> bool {
        ct_eq(&self.access_code, access_code)
    }

    pub async fn purge_expired_auth_requests(conn: &mut DbConn) {
        let expiry_time = Utc::now().naive_utc() - chrono::TimeDelta::try_minutes(5).unwrap(); //after 5 minutes, clients reject the request
        for auth_request in Self::find_created_before(&expiry_time, conn).await {
            auth_request.delete(conn).await.ok();
        }
    }
}

#[derive(
    Clone,
    Debug,
    AsRef,
    Deref,
    DieselNewType,
    Display,
    From,
    FromForm,
    Hash,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    UuidFromParam,
)]
pub struct AuthRequestId(String);
