// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

package main

import (
	"context"
	"iter"
	"net/http"

	"github.com/a2aproject/a2a-go/v2/a2a"
	"github.com/a2aproject/a2a-go/v2/a2asrv"
)

func card(name string) *a2a.AgentCard {
	return &a2a.AgentCard{
		Name: name, Description: "d", Version: "1.0.0",
		SupportedInterfaces: []*a2a.AgentInterface{a2a.NewAgentInterface("http://127.0.0.1:7631/", a2a.TransportProtocolJSONRPC)},
		Capabilities:        a2a.AgentCapabilities{ExtendedAgentCard: true},
		DefaultInputModes:   []string{"text/plain"}, DefaultOutputModes: []string{"text/plain"},
		Skills: []a2a.AgentSkill{{ID: "s", Name: "s", Description: "s", Tags: []string{"t"}}},
	}
}

func main() {
	exec := a2asrv.AgentExecutorFunc(func(ctx context.Context, ec *a2asrv.ExecutorContext) iter.Seq2[a2a.Event, error] {
		return func(yield func(a2a.Event, error) bool) {}
	})
	pub := card("go-public")
	h := a2asrv.NewHandler(exec, a2asrv.WithExtendedAgentCard(card("go-EXTENDED")), a2asrv.WithCapabilityChecks(&pub.Capabilities))
	mux := http.NewServeMux()
	mux.Handle("/", a2asrv.NewJSONRPCHandler(h))
	mux.Handle(a2asrv.WellKnownAgentCardPath, a2asrv.NewStaticAgentCardHandler(pub))
	http.ListenAndServe("127.0.0.1:7631", mux)
}
