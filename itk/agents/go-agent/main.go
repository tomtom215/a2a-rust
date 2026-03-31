// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

// A2A ITK — Go Echo Agent
//
// A minimal A2A agent in Go that echoes incoming messages.
// Used for cross-language interoperability testing with the Rust A2A SDK.
//
// Usage:
//
//	go run . [--port 9102]
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"net/http"
	"strings"
	"sync"

	"github.com/google/uuid"
)

var (
	port       = flag.Int("port", 9102, "Port to listen on")
	tasks      = sync.Map{}
	pushCfgs   = sync.Map{}
)

// --- Types ---

type Part struct {
	Text string `json:"text,omitempty"`
}

type Message struct {
	Role      string `json:"role"`
	Parts     []Part `json:"parts"`
	MessageID string `json:"messageId"`
}

type TaskStatus struct {
	State string `json:"state"`
}

type Artifact struct {
	ArtifactID string `json:"artifactId"`
	Parts      []Part `json:"parts"`
}

type Task struct {
	ID        string     `json:"id"`
	ContextID string     `json:"contextId"`
	Status    TaskStatus `json:"status"`
	History   []Message  `json:"history,omitempty"`
	Artifacts []Artifact `json:"artifacts,omitempty"`
}

type SendParams struct {
	Message   Message `json:"message"`
	ContextID string  `json:"contextId,omitempty"`
}

type PushConfig struct {
	ID     string `json:"id"`
	TaskID string `json:"taskId"`
	URL    string `json:"url"`
}

type JsonRpcRequest struct {
	Jsonrpc string          `json:"jsonrpc"`
	ID      json.RawMessage `json:"id"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params"`
}

type JsonRpcResponse struct {
	Jsonrpc string      `json:"jsonrpc"`
	ID      json.RawMessage `json:"id"`
	Result  interface{} `json:"result,omitempty"`
	Error   *RpcError   `json:"error,omitempty"`
}

type RpcError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

type TaskListResponse struct {
	Tasks []Task `json:"tasks"`
}

type SendMessageResponse struct {
	Task Task `json:"task"`
}

// Agent Card
type AgentCard struct {
	Name               string      `json:"name"`
	Description        string      `json:"description"`
	URL                string      `json:"url"`
	Version            string      `json:"version"`
	Capabilities        interface{} `json:"capabilities"`
	SupportedInterfaces []string   `json:"supportedInterfaces"`
	DefaultInputModes  []string    `json:"defaultInputModes"`
	DefaultOutputModes []string    `json:"defaultOutputModes"`
	Skills             []Skill     `json:"skills"`
}

type Skill struct {
	ID          string   `json:"id"`
	Name        string   `json:"name"`
	Description string   `json:"description"`
	Tags        []string `json:"tags"`
}

// responseRecorder captures the status code and body from the inner handler.
type responseRecorder struct {
	http.ResponseWriter
	statusCode int
	body       []byte
	written    bool
}

func (r *responseRecorder) WriteHeader(code int) {
	r.statusCode = code
	r.written = true
}

func (r *responseRecorder) Write(b []byte) (int, error) {
	r.body = append(r.body, b...)
	return len(b), nil
}

// jsonNotFoundHandler wraps an http.Handler and returns JSON for 404/405 responses.
func jsonNotFoundHandler(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		rec := &responseRecorder{ResponseWriter: w, statusCode: 200}
		next.ServeHTTP(rec, r)
		if rec.written && (rec.statusCode == 404 || rec.statusCode == 405) {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(rec.statusCode)
			json.NewEncoder(w).Encode(RpcError{Code: -32601, Message: "Not found"})
			return
		}
		if rec.written {
			w.WriteHeader(rec.statusCode)
		}
		w.Write(rec.body)
	})
}

func extractText(msg Message) string {
	var texts []string
	for _, p := range msg.Parts {
		if p.Text != "" {
			texts = append(texts, p.Text)
		}
	}
	return strings.Join(texts, "")
}

func processMessage(params SendParams) Task {
	text := extractText(params.Message)
	taskID := uuid.New().String()
	contextID := params.ContextID
	if contextID == "" {
		contextID = uuid.New().String()
	}

	task := Task{
		ID:        taskID,
		ContextID: contextID,
		Status:    TaskStatus{State: "TASK_STATE_COMPLETED"},
		History:   []Message{params.Message},
		Artifacts: []Artifact{{
			ArtifactID: uuid.New().String(),
			Parts:      []Part{{Text: fmt.Sprintf("[Go Echo] %s", text)}},
		}},
	}

	tasks.Store(taskID, task)
	return task
}

func handleJsonRpc(w http.ResponseWriter, r *http.Request) {
	var req JsonRpcRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeJsonRpc(w, req.ID, nil, &RpcError{Code: -32700, Message: "Parse error"})
		return
	}

	switch req.Method {
	case "SendMessage", "SendStreamingMessage":
		var params SendParams
		if err := json.Unmarshal(req.Params, &params); err != nil {
			writeJsonRpc(w, req.ID, nil, &RpcError{Code: -32602, Message: "Invalid params"})
			return
		}
		if params.Message.Parts == nil && params.Message.Role == "" {
			writeJsonRpc(w, req.ID, nil, &RpcError{Code: -32602, Message: "Invalid params: missing message"})
			return
		}
		task := processMessage(params)
		writeJsonRpc(w, req.ID, SendMessageResponse{Task: task}, nil)

	case "GetTask":
		var params struct{ ID string `json:"id"` }
		json.Unmarshal(req.Params, &params)
		if val, ok := tasks.Load(params.ID); ok {
			writeJsonRpc(w, req.ID, val, nil)
		} else {
			writeJsonRpc(w, req.ID, nil, &RpcError{Code: -32001, Message: "Task not found"})
		}

	case "ListTasks":
		var allTasks []Task
		tasks.Range(func(_, v interface{}) bool {
			allTasks = append(allTasks, v.(Task))
			return true
		})
		if allTasks == nil {
			allTasks = []Task{}
		}
		writeJsonRpc(w, req.ID, TaskListResponse{Tasks: allTasks}, nil)

	case "CancelTask":
		var params struct{ ID string `json:"id"` }
		json.Unmarshal(req.Params, &params)
		if val, ok := tasks.Load(params.ID); ok {
			task := val.(Task)
			task.Status = TaskStatus{State: "TASK_STATE_CANCELED"}
			tasks.Store(params.ID, task)
			writeJsonRpc(w, req.ID, task, nil)
		} else {
			writeJsonRpc(w, req.ID, nil, &RpcError{Code: -32001, Message: "Task not found"})
		}

	case "CreateTaskPushNotificationConfig":
		var cfg PushConfig
		json.Unmarshal(req.Params, &cfg)
		if cfg.ID == "" {
			cfg.ID = uuid.New().String()
		}
		key := cfg.TaskID + ":" + cfg.ID
		pushCfgs.Store(key, cfg)
		writeJsonRpc(w, req.ID, cfg, nil)

	case "GetTaskPushNotificationConfig":
		var params struct {
			TaskID string `json:"taskId"`
			ID     string `json:"id"`
		}
		json.Unmarshal(req.Params, &params)
		key := params.TaskID + ":" + params.ID
		if val, ok := pushCfgs.Load(key); ok {
			writeJsonRpc(w, req.ID, val, nil)
		} else {
			writeJsonRpc(w, req.ID, nil, &RpcError{Code: -32001, Message: "Config not found"})
		}

	case "ListTaskPushNotificationConfigs":
		var params struct{ TaskID string `json:"taskId"` }
		json.Unmarshal(req.Params, &params)
		var configs []PushConfig
		pushCfgs.Range(func(_, v interface{}) bool {
			cfg := v.(PushConfig)
			if cfg.TaskID == params.TaskID {
				configs = append(configs, cfg)
			}
			return true
		})
		if configs == nil {
			configs = []PushConfig{}
		}
		writeJsonRpc(w, req.ID, configs, nil)

	case "DeleteTaskPushNotificationConfig":
		var params struct {
			TaskID string `json:"taskId"`
			ID     string `json:"id"`
		}
		json.Unmarshal(req.Params, &params)
		key := params.TaskID + ":" + params.ID
		pushCfgs.Delete(key)
		writeJsonRpc(w, req.ID, map[string]string{}, nil)

	default:
		writeJsonRpc(w, req.ID, nil, &RpcError{Code: -32601, Message: fmt.Sprintf("Method not found: %s", req.Method)})
	}
}

func writeJsonRpc(w http.ResponseWriter, id json.RawMessage, result interface{}, err *RpcError) {
	resp := JsonRpcResponse{Jsonrpc: "2.0", ID: id}
	if err != nil {
		resp.Error = err
	} else {
		resp.Result = result
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(resp)
}

func main() {
	flag.Parse()

	agentCard := AgentCard{
		Name:               "ITK Go Echo Agent",
		Description:        "A2A ITK echo agent implemented in Go",
		URL:                fmt.Sprintf("http://0.0.0.0:%d", *port),
		Version:            "1.0.0",
		Capabilities:        map[string]bool{"streaming": true, "pushNotifications": true},
		SupportedInterfaces: []string{"a2a"},
		DefaultInputModes:  []string{"text"},
		DefaultOutputModes: []string{"text"},
		Skills: []Skill{{
			ID:          "echo",
			Name:        "Echo",
			Description: "Echoes the input message back",
			Tags:        []string{"echo", "test"},
		}},
	}

	mux := http.NewServeMux()

	// Agent card
	mux.HandleFunc("GET /.well-known/agent-card.json", func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(agentCard)
	})

	// JSON-RPC endpoint
	mux.HandleFunc("POST /", handleJsonRpc)

	// REST endpoints
	mux.HandleFunc("POST /message/send", func(w http.ResponseWriter, r *http.Request) {
		var params SendParams
		json.NewDecoder(r.Body).Decode(&params)
		task := processMessage(params)
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(SendMessageResponse{Task: task})
	})

	mux.HandleFunc("POST /message/stream", func(w http.ResponseWriter, r *http.Request) {
		var params SendParams
		json.NewDecoder(r.Body).Decode(&params)
		task := processMessage(params)
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(SendMessageResponse{Task: task})
	})

	mux.HandleFunc("GET /tasks", func(w http.ResponseWriter, _ *http.Request) {
		var allTasks []Task
		tasks.Range(func(_, v interface{}) bool {
			allTasks = append(allTasks, v.(Task))
			return true
		})
		if allTasks == nil {
			allTasks = []Task{}
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(TaskListResponse{Tasks: allTasks})
	})

	// REST: Get task by ID
	mux.HandleFunc("GET /tasks/{id}", func(w http.ResponseWriter, r *http.Request) {
		taskID := r.PathValue("id")
		if val, ok := tasks.Load(taskID); ok {
			w.Header().Set("Content-Type", "application/json")
			json.NewEncoder(w).Encode(val)
		} else {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(404)
			json.NewEncoder(w).Encode(RpcError{Code: -32001, Message: "Task not found"})
		}
	})

	// REST: Cancel task
	mux.HandleFunc("POST /tasks/{id}/cancel", func(w http.ResponseWriter, r *http.Request) {
		taskID := r.PathValue("id")
		if val, ok := tasks.Load(taskID); ok {
			task := val.(Task)
			task.Status = TaskStatus{State: "TASK_STATE_CANCELED"}
			tasks.Store(taskID, task)
			w.Header().Set("Content-Type", "application/json")
			json.NewEncoder(w).Encode(task)
		} else {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(404)
			json.NewEncoder(w).Encode(RpcError{Code: -32001, Message: "Task not found"})
		}
	})

	// REST: Create push notification config
	mux.HandleFunc("POST /tasks/{taskId}/pushNotificationConfig", func(w http.ResponseWriter, r *http.Request) {
		taskID := r.PathValue("taskId")
		var cfg PushConfig
		json.NewDecoder(r.Body).Decode(&cfg)
		cfg.TaskID = taskID
		if cfg.ID == "" {
			cfg.ID = uuid.New().String()
		}
		key := taskID + ":" + cfg.ID
		pushCfgs.Store(key, cfg)
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(cfg)
	})

	// REST: Get push notification config by id
	mux.HandleFunc("GET /tasks/{taskId}/pushNotificationConfig/{configId}", func(w http.ResponseWriter, r *http.Request) {
		key := r.PathValue("taskId") + ":" + r.PathValue("configId")
		if val, ok := pushCfgs.Load(key); ok {
			w.Header().Set("Content-Type", "application/json")
			json.NewEncoder(w).Encode(val)
		} else {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(404)
			json.NewEncoder(w).Encode(RpcError{Code: -32001, Message: "Config not found"})
		}
	})

	// REST: List push notification configs for task
	mux.HandleFunc("GET /tasks/{taskId}/pushNotificationConfig", func(w http.ResponseWriter, r *http.Request) {
		taskID := r.PathValue("taskId")
		var configs []PushConfig
		pushCfgs.Range(func(_, v interface{}) bool {
			cfg := v.(PushConfig)
			if cfg.TaskID == taskID {
				configs = append(configs, cfg)
			}
			return true
		})
		if configs == nil {
			configs = []PushConfig{}
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(configs)
	})

	// REST: Delete push notification config
	mux.HandleFunc("POST /tasks/{taskId}/pushNotificationConfig/{configId}/delete", func(w http.ResponseWriter, r *http.Request) {
		key := r.PathValue("taskId") + ":" + r.PathValue("configId")
		pushCfgs.Delete(key)
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{})
	})

	addr := fmt.Sprintf("0.0.0.0:%d", *port)
	log.Printf("[ITK] Go Echo Agent listening on port %d", *port)
	// Wrap mux to return JSON for unhandled routes instead of plain text
	log.Fatal(http.ListenAndServe(addr, jsonNotFoundHandler(mux)))
}
