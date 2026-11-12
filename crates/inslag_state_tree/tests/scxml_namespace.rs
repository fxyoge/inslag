use inslag_state_tree::scxml::{INSLAG_STATE_TREE_NAMESPACE, compile, parse};

#[test]
fn extension_prefixes_resolve_to_the_inslag_namespace() {
    assert_eq!(INSLAG_STATE_TREE_NAMESPACE, "urn:inslag-state-tree");
    for prefix in ["ist", "custom"] {
        let source = format!(
            r#"<scxml xmlns="http://www.w3.org/2005/07/scxml"
                      xmlns:{prefix}="urn:inslag-state-tree" initial="active">
                <state id="active">
                    <onentry><{prefix}:effect name="record"/></onentry>
                    <{prefix}:activity id="child" name="ist.spawn"/>
                    <transition {prefix}:after="1" target="done"/>
                </state>
                <final id="done"/>
            </scxml>"#
        );
        compile(&parse("namespace.scxml", &source).unwrap()).unwrap();
    }
}

#[test]
fn ist_prefix_does_not_make_a_foreign_namespace_supported() {
    let source = r#"<scxml xmlns="http://www.w3.org/2005/07/scxml"
                          xmlns:ist="urn:unrelated" initial="active">
        <state id="active"><onentry><ist:effect name="record"/></onentry></state>
    </scxml>"#;
    assert!(compile(&parse("foreign.scxml", source).unwrap()).is_err());
}
