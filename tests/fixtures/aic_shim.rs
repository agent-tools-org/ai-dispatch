// Shared shell fixture for the cross-auditor command contract.
// Exports script builders used by unit and integration tests.

pub fn script(body: &str) -> String {
    identified_script("aic 0.3.1", "Cross-audit aid agent outputs", body)
}

pub fn identified_script(version: &str, help: &str, body: &str) -> String {
    format!(
        "#!/bin/sh\ncase \"$1\" in\n\
         --version) printf '%s\\n' '{version}'; exit 0 ;;\n\
         --help) printf '%s\\n' '{help}'; exit 0 ;;\n\
         esac\n{body}\n"
    )
}
