#!/usr/bin/env python3
import os
import re

files = [
    'tests/phase2.rs', 'tests/phase15.rs', 
    'tests/integration_delegation.rs', 'tests/integration_guard_enforcement.rs',
    'tests/integration_pipeline_runner.rs', 'tests/integration_tool_execution.rs',
    'verdict-app/tests/agent_tests.rs', 'verdict-app/tests/pipeline_tests.rs'
]

for file_path in files:
    if not os.path.exists(file_path):
        print(f"File not found: {file_path}")
        continue
    
    with open(file_path, 'r') as f:
        content = f.read()
    
    # Replace pattern: policy: Default::default(),\n    }; with policy: Default::default(),\n        scorers: vec![],\n    };
    pattern = r'(policy: Default::default\(\),)\s*(\n\s*};)'
    replacement = r'\1\n        scorers: vec![],\2'
    
    new_content = re.sub(pattern, replacement, content)
    
    # Count replacements
    count = len(re.findall(pattern, content))
    
    with open(file_path, 'w') as f:
        f.write(new_content)
    
    print(f"Fixed {file_path}: {count} replacements")

print("Done!")
