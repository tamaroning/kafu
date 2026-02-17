use kafu_config::KafuConfig;

#[test]
fn test_parse_and_validate() {
    let config = KafuConfig::load("tests/fixtures/basic.yaml").unwrap();
    assert_eq!(config.name, "basic");
    assert_eq!(config.app.args, vec!["foo", "bar", "baz"]);
    assert_eq!(config.nodes.len(), 3);
    assert_eq!(config.nodes.get("node1").unwrap().address, "127.0.0.1");
    assert_eq!(config.nodes.get("node1").unwrap().port, 50051);
    assert_eq!(config.nodes.get("node2").unwrap().address, "127.0.0.1");
    assert_eq!(config.nodes.get("node2").unwrap().port, 50052);
    assert_eq!(config.nodes.get("node3").unwrap().address, "127.0.0.1");
    assert_eq!(config.nodes.get("node3").unwrap().port, 50053);
}

#[test]
fn test_empty_cluster() {
    let config = KafuConfig::load("tests/fixtures/empty_cluster.yaml").unwrap_err();
    assert_eq!(config, "At least one node is required in the nodes field");
}
