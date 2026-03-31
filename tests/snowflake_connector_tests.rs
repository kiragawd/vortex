// Snowflake connector unit tests (no real Snowflake connection needed)

use vortex::connectors::SnowflakeConnector;
use vortex::enterprise_connector::{ConnectorCapability, EnterpriseConnector};

#[test]
fn test_snowflake_connector_creation() {
    let conn = SnowflakeConnector::new("test-account")
        .with_warehouse("wh")
        .with_database("db")
        .with_schema("public");
    assert!(conn.capabilities().contains(&ConnectorCapability::BatchRead));
    assert!(conn.capabilities().contains(&ConnectorCapability::BatchWrite));
}

#[test]
fn test_snowflake_connector_builder_warehouse() {
    let conn = SnowflakeConnector::new("acct")
        .with_warehouse("analytics_wh");
    assert_eq!(conn.warehouse, Some("analytics_wh".to_string()));
}

#[test]
fn test_snowflake_connector_builder_database_schema() {
    let conn = SnowflakeConnector::new("acct")
        .with_database("mydb")
        .with_schema("myschema");
    assert_eq!(conn.database, Some("mydb".to_string()));
    assert_eq!(conn.schema, Some("myschema".to_string()));
}

#[test]
fn test_snowflake_connector_builder_role() {
    let conn = SnowflakeConnector::new("acct")
        .with_role("SYSADMIN");
    assert_eq!(conn.role, Some("SYSADMIN".to_string()));
}

#[test]
fn test_snowflake_connector_with_password_auth() {
    let conn = SnowflakeConnector::new("acct")
        .with_password_auth();
    assert!(!conn.capabilities().is_empty());
}

#[test]
fn test_snowflake_connector_with_keypair_auth() {
    let conn = SnowflakeConnector::new("acct")
        .with_keypair_auth("user", "-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----");
    assert!(!conn.capabilities().is_empty());
}

#[test]
fn test_snowflake_connector_with_snowsql_transport() {
    let conn = SnowflakeConnector::new("acct")
        .with_snowsql_transport();
    assert!(!conn.capabilities().is_empty());
}

#[test]
fn test_snowflake_connector_chained_builder() {
    let conn = SnowflakeConnector::new("acct")
        .with_warehouse("big_wh")
        .with_database("analytics")
        .with_schema("raw")
        .with_role("LOADER")
        .with_keypair_auth("svc_user", "fake_key")
        .with_snowsql_transport();
    assert_eq!(conn.account, "acct");
    assert_eq!(conn.warehouse, Some("big_wh".to_string()));
    assert_eq!(conn.database, Some("analytics".to_string()));
    assert_eq!(conn.role, Some("LOADER".to_string()));
    assert!(!conn.capabilities().is_empty());
}
