// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
//
// A2A echo agent built on the OFFICIAL Go SDK (github.com/a2aproject/a2a-go/v2).
//
// Unlike itk/agents/go-agent (a dependency-light stub that hand-writes the
// wire format), this agent is assembled from the official SDK's server
// framework — a2asrv.NewHandler, the JSON-RPC and REST transports — so
// running our TCK against it validates this Rust SDK's wire expectations
// against the reference Go implementation.
//
// Behavior contract (same as every ITK echo agent): SendMessage returns a
// completed task whose artifact echoes the input text as "Echo: <text>".
//
// Run: go run .    Env: PORT (default 9112).
package main

import (
	"context"
	"fmt"
	"iter"
	"log"
	"net/http"
	"os"
	"strings"

	"github.com/a2aproject/a2a-go/v2/a2a"
	"github.com/a2aproject/a2a-go/v2/a2asrv"
	"github.com/a2aproject/a2a-go/v2/a2asrv/push"
	"github.com/a2aproject/a2a-go/v2/a2asrv/taskstore"
)

type echoExecutor struct{}

func (e *echoExecutor) Execute(_ context.Context, execCtx *a2asrv.ExecutorContext) iter.Seq2[a2a.Event, error] {
	return func(yield func(a2a.Event, error) bool) {
		// The first event for a fresh task must be the Task itself.
		if execCtx.StoredTask == nil {
			if !yield(a2a.NewSubmittedTask(execCtx, execCtx.Message), nil) {
				return
			}
		}
		if !yield(a2a.NewStatusUpdateEvent(execCtx, a2a.TaskStateWorking, nil), nil) {
			return
		}

		var texts []string
		if execCtx.Message != nil {
			for _, part := range execCtx.Message.Parts {
				if text, ok := part.Content.(a2a.Text); ok {
					texts = append(texts, string(text))
				}
			}
		}
		echo := "Echo: " + strings.Join(texts, "\n")

		if !yield(a2a.NewArtifactEvent(execCtx, a2a.NewTextPart(echo)), nil) {
			return
		}
		yield(a2a.NewStatusUpdateEvent(execCtx, a2a.TaskStateCompleted, nil), nil)
	}
}

func (e *echoExecutor) Cancel(_ context.Context, execCtx *a2asrv.ExecutorContext) iter.Seq2[a2a.Event, error] {
	return func(yield func(a2a.Event, error) bool) {
		yield(a2a.NewStatusUpdateEvent(execCtx, a2a.TaskStateCanceled, nil), nil)
	}
}

func main() {
	port := os.Getenv("PORT")
	if port == "" {
		port = "9112"
	}
	baseURL := "http://127.0.0.1:" + port

	card := &a2a.AgentCard{
		Name:        "official-go-echo",
		Description: "Echo agent built on the official a2a-go/v2 SDK (Go)",
		Version:     "1.0.0",
		Capabilities: a2a.AgentCapabilities{
			Streaming:         true,
			PushNotifications: true,
		},
		DefaultInputModes:  []string{"text/plain"},
		DefaultOutputModes: []string{"text/plain"},
		Skills: []a2a.AgentSkill{
			{
				ID:          "echo",
				Name:        "Echo",
				Description: "Echoes back the input text",
				Tags:        []string{"echo", "test"},
			},
		},
		SupportedInterfaces: []*a2a.AgentInterface{
			a2a.NewAgentInterface(baseURL, a2a.TransportProtocolJSONRPC),
			a2a.NewAgentInterface(baseURL, a2a.TransportProtocolHTTPJSON),
		},
	}

	// The default handler wires a task-store authenticator that rejects
	// unauthenticated ListTasks; the ITK runs anonymously, so use the
	// store's permissive default authenticator instead, and enable the
	// push-notification config surface.
	handler := a2asrv.NewHandler(
		&echoExecutor{},
		// The in-memory store's List rejects empty usernames outright, so
		// anonymous ITK/TCK traffic runs under a fixed identity.
		a2asrv.WithTaskStore(taskstore.NewInMemory(&taskstore.InMemoryStoreConfig{
			Authenticator: func(context.Context) (string, error) { return "itk-anonymous", nil },
		})),
		a2asrv.WithPushNotifications(push.NewInMemoryStore(), push.NewHTTPPushSender(nil)),
	)
	jsonrpcHandler := a2asrv.NewJSONRPCHandler(handler)
	restHandler := a2asrv.NewRESTHandler(handler)
	cardHandler := a2asrv.NewStaticAgentCardHandler(card)

	mux := http.NewServeMux()
	mux.Handle(a2asrv.WellKnownAgentCardPath, cardHandler)
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		// JSON-RPC lives at the root path exactly; every other path is the
		// REST binding — mirrors our Rust combined server's routing.
		if r.URL.Path == "/" {
			jsonrpcHandler.ServeHTTP(w, r)
			return
		}
		restHandler.ServeHTTP(w, r)
	})

	fmt.Printf("official-go-echo listening on %s\n", baseURL)
	log.Fatal(http.ListenAndServe("127.0.0.1:"+port, mux))
}
