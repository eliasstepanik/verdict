#[cfg(test)]
mod tests {
    use crate::llm::{ChatMessage, ChatRole, MessageHistory};
    
    #[test]
    fn test_chatmessage_constructors() {
        // Test plain constructors
        let user_msg = ChatMessage::user("hello".to_string());
        assert_eq!(user_msg.role, ChatRole::User);
        assert_eq!(user_msg.content, "hello");
        assert!(user_msg.tool_calls_json.is_none());
        assert!(user_msg.tool_call_id.is_none());
        
        let asst_msg = ChatMessage::assistant("response".to_string());
        assert_eq!(asst_msg.role, ChatRole::Assistant);
        assert_eq!(asst_msg.content, "response");
        assert!(asst_msg.tool_calls_json.is_none());
        assert!(asst_msg.tool_call_id.is_none());
        
        // Test assistant with tool calls
        let tool_calls = serde_json::json!([
            {
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "fs_list",
                    "arguments": "{\"path\":\".\"}".to_string()
                }
            }
        ]);
        let tool_msg = ChatMessage::assistant_with_tool_calls("".to_string(), tool_calls.clone());
        assert_eq!(tool_msg.role, ChatRole::Assistant);
        assert!(tool_msg.tool_calls_json.is_some());
        assert_eq!(tool_msg.tool_calls_json, Some(tool_calls));
        
        // Test tool result
        let result_msg = ChatMessage::tool_result("call_1".to_string(), "[\"file1.txt\"]".to_string());
        assert_eq!(result_msg.role, ChatRole::Tool);
        assert_eq!(result_msg.content, "[\"file1.txt\"]");
        assert_eq!(result_msg.tool_call_id, Some("call_1".to_string()));
        assert!(result_msg.tool_calls_json.is_none());
    }
    
    #[test]
    fn test_message_history_push_initializes_new_fields() {
        let mut history = MessageHistory::new();
        history.push(ChatRole::User, "test".to_string());
        
        assert_eq!(history.messages.len(), 1);
        let msg = &history.messages[0];
        assert_eq!(msg.role, ChatRole::User);
        assert_eq!(msg.content, "test");
        assert!(msg.tool_calls_json.is_none());
        assert!(msg.tool_call_id.is_none());
    }
    
    #[test]
    fn test_tool_use_loop_proper_message_format() {
        // Simulates what ToolUseLoop does: build tool_calls JSON and store it
        let tool_calls_json = serde_json::json!([
            {
                "id": "call_0",
                "type": "function",
                "function": {
                    "name": "fs_list",
                    "arguments": r#"{"path":"."}"#
                }
            }
        ]);
        
        // Create assistant message with tool calls
        let asst_with_tools = ChatMessage::assistant_with_tool_calls(
            String::new(),  // empty content when tool_calls present
            tool_calls_json.clone()
        );
        
        assert_eq!(asst_with_tools.role, ChatRole::Assistant);
        assert!(asst_with_tools.content.is_empty());
        assert_eq!(asst_with_tools.tool_calls_json, Some(tool_calls_json));
        assert!(asst_with_tools.tool_call_id.is_none());
        
        // Create tool result message
        let tool_result = ChatMessage::tool_result(
            "call_0".to_string(),
            "[\"Cargo.toml\", \"src/\"]".to_string()
        );
        
        assert_eq!(tool_result.role, ChatRole::Tool);
        assert_eq!(tool_result.content, "[\"Cargo.toml\", \"src/\"]");
        assert_eq!(tool_result.tool_call_id, Some("call_0".to_string()));
        assert!(tool_result.tool_calls_json.is_none());
    }
}
