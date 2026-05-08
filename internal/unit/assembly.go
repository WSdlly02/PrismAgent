package unit

import (
	"encoding/json"
	"fmt"

	"prismagent/internal/atom"
	"prismagent/internal/core"
	"prismagent/internal/model"
)

// LLMResponse represents the DeepSeek/OpenAI chat completion response format
// for parsing the raw LLM response JSON stored in atoms.
type LLMResponse struct {
	Choices []struct {
		Message struct {
			Content          string        `json:"content"`
			ReasoningContent string        `json:"reasoning_content"`
			ToolCalls        []LLMToolCall `json:"tool_calls"`
		} `json:"message"`
	} `json:"choices"`
	Model string `json:"model,omitempty"`
}

// LLMToolCall represents a tool call in the OpenAI response format.
type LLMToolCall struct {
	ID       string `json:"id"`
	Function struct {
		Name      string `json:"name"`
		Arguments string `json:"arguments"`
	} `json:"function"`
}

// AssembleMessages converts an ordered slice of Units into model.Messages
// suitable for sending to an LLM. The atomStore is used to fetch raw content.
func AssembleMessages(chain []core.Unit, atomStore *atom.Store) ([]model.Message, error) {
	var messages []model.Message

	for _, u := range chain {
		switch {
		case u.Kind == core.UnitMessage:
			atomData, err := atomStore.Get(u.AtomHash)
			if err != nil {
				return nil, fmt.Errorf("assembly: get atom %s: %w", u.AtomHash, err)
			}
			messages = append(messages, model.Message{
				Role:    string(u.Role),
				Content: string(atomData),
			})

		case u.Kind == core.UnitLLMResp:
			atomData, err := atomStore.Get(u.AtomHash)
			if err != nil {
				return nil, fmt.Errorf("assembly: get atom %s: %w", u.AtomHash, err)
			}
			msg, err := parseLLMResponse(atomData)
			if err != nil {
				return nil, fmt.Errorf("assembly: parse llm response %s: %w", u.AtomHash, err)
			}
			messages = append(messages, msg)

		case u.Kind == core.UnitToolCall:
			// Skip: tool call info is already embedded in the LLM response atom.
			// This unit exists for audit/UI purposes only.
			continue

		case u.Kind == core.UnitSpawn:
			// Skip: spawn intent is recorded for audit; sub-agent runs in isolation.
			continue

		case u.Kind == core.UnitResult:
			// Skip: sub-agent result is recorded for audit; parent sees result via tool_result.
			continue

		case u.Kind == core.UnitToolResult:
			atomData, err := atomStore.Get(u.AtomHash)
			if err != nil {
				return nil, fmt.Errorf("assembly: get atom %s: %w", u.AtomHash, err)
			}
			toolCallID := ""
			if u.Metadata != nil {
				toolCallID = u.Metadata["tool_call_id"]
			}
			messages = append(messages, model.Message{
				Role:       "tool",
				ToolCallID: toolCallID,
				Content:    string(atomData),
			})

		default:
			// Skip unknown unit kinds (e.g. UnitSpawn, UnitResult)
			continue
		}
	}

	return messages, nil
}

// parseLLMResponse parses raw LLM API response JSON into a model.Message.
func parseLLMResponse(data []byte) (model.Message, error) {
	var resp LLMResponse
	if err := json.Unmarshal(data, &resp); err != nil {
		return model.Message{}, fmt.Errorf("unmarshal llm response: %w", err)
	}

	if len(resp.Choices) == 0 {
		return model.Message{}, fmt.Errorf("llm response has no choices")
	}

	msg := resp.Choices[0].Message
	result := model.Message{
		Role:             "assistant",
		Content:          msg.Content,
		ReasoningContent: msg.ReasoningContent,
	}

	for _, tc := range msg.ToolCalls {
		result.ToolCalls = append(result.ToolCalls, model.ToolCall{
			ID:           tc.ID,
			Name:         tc.Function.Name,
			RawArguments: tc.Function.Arguments,
		})
	}

	return result, nil
}

// ParseToolCalls extracts tool calls from an OpenAI-format LLM response atom.
// Returns nil if the data is not valid or contains no tool calls.
func ParseToolCalls(data []byte) []model.ToolCall {
	var resp LLMResponse
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil
	}
	if len(resp.Choices) == 0 {
		return nil
	}
	var calls []model.ToolCall
	for _, tc := range resp.Choices[0].Message.ToolCalls {
		call := model.ToolCall{
			ID:           tc.ID,
			Name:         tc.Function.Name,
			RawArguments: tc.Function.Arguments,
		}
		var args map[string]string
		if err := json.Unmarshal([]byte(tc.Function.Arguments), &args); err == nil {
			call.Arguments = args
		}
		calls = append(calls, call)
	}
	return calls
}

// BuildLLMAtom serializes a model.Response into OpenAI-format JSON bytes
// suitable for storage as an LLM response atom.
func BuildLLMAtom(resp model.Response) []byte {
	atom := LLMResponse{
		Choices: []struct {
			Message struct {
				Content          string        `json:"content"`
				ReasoningContent string        `json:"reasoning_content"`
				ToolCalls        []LLMToolCall `json:"tool_calls"`
			} `json:"message"`
		}{{
			Message: struct {
				Content          string        `json:"content"`
				ReasoningContent string        `json:"reasoning_content"`
				ToolCalls        []LLMToolCall `json:"tool_calls"`
			}{
				Content:          resp.Text,
				ReasoningContent: resp.ReasoningContent,
			},
		}},
		Model: resp.Model,
	}
	for _, tc := range resp.ToolCalls {
		atom.Choices[0].Message.ToolCalls = append(atom.Choices[0].Message.ToolCalls, LLMToolCall{
			ID: tc.ID,
			Function: struct {
				Name      string `json:"name"`
				Arguments string `json:"arguments"`
			}{
				Name:      tc.Name,
				Arguments: tc.RawArguments,
			},
		})
	}
	data, _ := json.Marshal(atom)
	return data
}
