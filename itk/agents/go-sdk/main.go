// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
//
// A2A echo agent built on the OFFICIAL Go SDK (github.com/a2aproject/a2a-go/v2).
//
// Unlike itk/agents/go-agent (a dependency-light stub that hand-writes the
// wire format), this agent is assembled from the official SDK's server
// framework — a2asrv.NewHandler, the JSON-RPC, REST and gRPC transports — so
// running our TCK and our client against it validates this Rust SDK's wire
// expectations against the reference Go implementation.
//
// Behavior contract (same as every ITK echo agent): SendMessage returns a
// completed task whose artifact echoes the input text as "Echo: <text>".
// A message whose text starts with "slow:" holds the task in WORKING for
// slowTurn before completing, so a caller can subscribe to, or cancel, a
// task that is still running. Cancellation interrupts the wait.
//
// Environment:
//
//	PORT       HTTP port for JSON-RPC (at /) and HTTP+JSON (default 9112).
//	GRPC_PORT  when set, also serves lf.a2a.v1.A2AService on this port and
//	           lists it in the card.
//	INTEROP_CARD=1
//	           publishes the card shapes that the Go/Rust interop gate
//	           (scripts/go_sdk_interop.sh) must survive and that the TCK leg
//	           deliberately does not see: card- and skill-level
//	           securityRequirements in a2a-go's own JSON shape, and a v0.3
//	           JSONRPC interface listed FIRST at an address nothing serves, so
//	           a client that ignores protocolVersion picks it and fails.
//	           Also configures the extended agent card, so all eleven methods
//	           have a success path, and lets the push sender reach 127.0.0.1
//	           webhooks.
//
// Run: go run .
package main

import (
	"context"
	"fmt"
	"iter"
	"log"
	"net"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/a2aproject/a2a-go/v2/a2a"
	grpcv1 "github.com/a2aproject/a2a-go/v2/a2agrpc/v1"
	"github.com/a2aproject/a2a-go/v2/a2asrv"
	"github.com/a2aproject/a2a-go/v2/a2asrv/push"
	"github.com/a2aproject/a2a-go/v2/a2asrv/taskstore"
	"google.golang.org/grpc"
)

// slowTurn is long enough for a client to open a second connection and
// subscribe or cancel, and short enough not to dominate a CI job.
const slowTurn = 1500 * time.Millisecond

type echoExecutor struct{}

func (e *echoExecutor) Execute(ctx context.Context, execCtx *a2asrv.ExecutorContext) iter.Seq2[a2a.Event, error] {
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
		joined := strings.Join(texts, "\n")
		if strings.HasPrefix(joined, "slow:") {
			select {
			case <-time.After(slowTurn):
			case <-ctx.Done():
				return
			}
		}

		if !yield(a2a.NewArtifactEvent(execCtx, a2a.NewTextPart("Echo: "+joined)), nil) {
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
	grpcPort := os.Getenv("GRPC_PORT")
	interopCard := os.Getenv("INTEROP_CARD") == "1"
	baseURL := "http://127.0.0.1:" + port

	skill := a2a.AgentSkill{
		ID:          "echo",
		Name:        "Echo",
		Description: "Echoes back the input text",
		Tags:        []string{"echo", "test"},
	}
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
		SupportedInterfaces: []*a2a.AgentInterface{
			a2a.NewAgentInterface(baseURL, a2a.TransportProtocolJSONRPC),
			a2a.NewAgentInterface(baseURL, a2a.TransportProtocolHTTPJSON),
		},
	}
	if grpcPort != "" {
		// a2a-go's gRPC transport dials a bare host:port.
		card.SupportedInterfaces = append(card.SupportedInterfaces,
			a2a.NewAgentInterface("127.0.0.1:"+grpcPort, a2a.TransportProtocolGRPC))
	}
	if interopCard {
		// Advertised, not enforced: the gate is about whether a peer can
		// read the card, and enforcing it would need credentials on every
		// call the gate makes.
		card.SecuritySchemes = a2a.NamedSecuritySchemes{
			"apiKey": a2a.APIKeySecurityScheme{
				Location: a2a.APIKeySecuritySchemeLocationHeader,
				Name:     "X-API-Key",
			},
		}
		card.SecurityRequirements = a2a.SecurityRequirementsOptions{
			{"apiKey": a2a.SecuritySchemeScopes{"read", "write"}},
		}
		skill.SecurityRequirements = a2a.SecurityRequirementsOptions{
			{"apiKey": a2a.SecuritySchemeScopes{"read"}},
		}
		// Port 9 (discard) on loopback: nothing in CI serves A2A there, so
		// choosing this interface is a failed call rather than a quiet pass.
		decoy := &a2a.AgentInterface{
			URL:             "http://127.0.0.1:9/v03",
			ProtocolBinding: a2a.TransportProtocolJSONRPC,
			ProtocolVersion: "0.3",
		}
		card.SupportedInterfaces = append([]*a2a.AgentInterface{decoy}, card.SupportedInterfaces...)
	}
	card.Skills = []a2a.AgentSkill{skill}

	options := []a2asrv.RequestHandlerOption{
		// The in-memory store's List rejects empty usernames outright, so
		// anonymous ITK/TCK traffic runs under a fixed identity.
		a2asrv.WithTaskStore(taskstore.NewInMemory(&taskstore.InMemoryStoreConfig{
			Authenticator: func(context.Context) (string, error) { return "itk-anonymous", nil },
		})),
		a2asrv.WithPushNotifications(push.NewInMemoryStore(),
			push.NewHTTPPushSender(&push.HTTPSenderConfig{AllowPrivateNetworks: interopCard})),
	}
	if interopCard {
		// The interop gate drives all eleven methods, GetExtendedAgentCard
		// among them; without a configured card a2a-go answers it with
		// "extended card not configured", correctly.
		card.Capabilities.ExtendedAgentCard = true
		options = append(options, a2asrv.WithExtendedAgentCard(card))
	}

	handler := a2asrv.NewHandler(&echoExecutor{}, options...)
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

	if grpcPort != "" {
		listener, err := net.Listen("tcp", "127.0.0.1:"+grpcPort)
		if err != nil {
			log.Fatalf("grpc listen: %v", err)
		}
		server := grpc.NewServer()
		grpcv1.NewHandler(handler).RegisterWith(server)
		go func() {
			if err := server.Serve(listener); err != nil {
				log.Fatalf("grpc serve: %v", err)
			}
		}()
		fmt.Printf("official-go-echo gRPC on 127.0.0.1:%s\n", grpcPort)
	}

	fmt.Printf("official-go-echo listening on %s\n", baseURL)
	log.Fatal(http.ListenAndServe("127.0.0.1:"+port, mux))
}
