use tree_sitter::Node;

const MEANINGFUL_KINDS: &[&str] = &[
    "function_declaration",
    "method_definition",
    "arrow_function",
    "variable_declarator",
    "class_declaration",
    "call_expression",
    "binary_expression",
    "return_statement",
    "assignment_expression",
    "type_alias_declaration",
    "interface_declaration",
    "export_statement",
];

pub fn find_meaningful_enclosing(node: Node<'_>) -> Node<'_> {
    let mut current = node;
    loop {
        // Skip ERROR nodes by going to parent
        if current.kind() == "ERROR" {
            match current.parent() {
                Some(p) => {
                    current = p;
                    continue;
                }
                None => return current,
            }
        }

        if MEANINGFUL_KINDS.contains(&current.kind()) {
            return current;
        }

        match current.parent() {
            Some(p) => current = p,
            None => return current,
        }
    }
}

pub fn collect_named_scope_path(node: Node<'_>, source: &[u8]) -> Vec<String> {
    const SCOPE_KINDS: &[&str] = &[
        "function_declaration",
        "method_definition",
        "class_declaration",
        "variable_declarator",
        "function",
    ];

    let mut path = Vec::new();
    let mut current = node;

    loop {
        if SCOPE_KINDS.contains(&current.kind())
            && let Some(name_node) = current.child_by_field_name("name")
            && let Ok(text) = name_node.utf8_text(source)
        {
            path.push(text.to_string());
        }

        match current.parent() {
            Some(p) => current = p,
            None => break,
        }
    }

    path.reverse();
    path
}

pub fn compute_occurrence_index(node: Node<'_>, source: &[u8]) -> usize {
    let target_kind = node.kind();
    let target_text = normalize_whitespace(node.utf8_text(source).unwrap_or(""));

    let Some(parent) = node.parent() else {
        return 0;
    };

    let mut index = 0;
    let mut cursor = parent.walk();
    for sibling in parent.children(&mut cursor) {
        if sibling.id() == node.id() {
            break;
        }
        if sibling.kind() == target_kind {
            let sibling_text = normalize_whitespace(sibling.utf8_text(source).unwrap_or(""));
            if sibling_text == target_text {
                index += 1;
            }
        }
    }
    index
}

pub fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
