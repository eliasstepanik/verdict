#!/usr/bin/env python3
import os
import re

# Search for ALL Agent { patterns and add scorers
files_to_search = [
    'tests/phase4.rs',
    'tests/phase2.rs', 'tests/phase15.rs', 
    'tests/integration_delegation.rs', 'tests/integration_guard_enforcement.rs',
    'tests/integration_pipeline_runner.rs', 'tests/integration_tool_execution.rs',
    'verdict-app/tests/agent_tests.rs', 'verdict-app/tests/pipeline_tests.rs'
]

for file_path in files_to_search:
    if not os.path.exists(file_path):
        print(f"File not found: {file_path}")
        continue
    
    with open(file_path, 'r') as f:
        lines = f.readlines()
    
    modified = False
    i = 0
    while i < len(lines):
        # Look for policy: lines that don't already have scorers: on the next line
        if 'policy:' in lines[i] and 'Default::default()' in lines[i]:
            # Check if next line is closing }; and line after that is not scorers
            if i + 1 < len(lines) and '};' in lines[i + 1]:
                # Check if scorers: is not already there
                if i > 0 and 'scorers:' not in lines[i]:
                    # Insert scorers: line after policy:
                    indent = len(lines[i]) - len(lines[i].lstrip())
                    insert_line = ' ' * indent + 'scorers: vec![],\n'
                    lines.insert(i + 1, insert_line)
                    modified = True
                    i += 2  # Skip the inserted line
                    continue
        i += 1
    
    if modified:
        with open(file_path, 'w') as f:
            f.writelines(lines)
        print(f"Fixed {file_path}")
    else:
        print(f"No changes needed for {file_path}")

print("Done!")
