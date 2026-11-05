// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

package main

// The lineage extension (ADR 0034), written by hand from the published wire
// contract (book: "Swarm Extensions: the Wire Contract"; ADR 0049), as a Go
// hop in a delegation tree would write it: a carried lineage is recorded on
// the task, and a malformed one is refused as invalid params before any work.

import (
	"context"
	"encoding/json"
	"fmt"
	"reflect"
	"strings"

	"github.com/a2aproject/a2a-go/v2/a2a"
	"github.com/a2aproject/a2a-go/v2/a2aclient"
)

const (
	lineageURI = "https://a2a-rust.com/extensions/lineage/v1"
	lineageKey = "a2a-rust.com/lineage"
)

func withLineage(prompt string, lineage map[string]any) *a2a.SendMessageRequest {
	req := text(prompt)
	req.Message.Metadata = map[string]any{lineageKey: lineage}
	req.Message.Extensions = []string{lineageURI}
	return req
}

func runLineage(ctx context.Context, s *suite, c *a2aclient.Client, binding a2a.TransportProtocol) {
	carried := map[string]any{"root": fmt.Sprintf("go-job-%s", binding), "depth": 2, "parentTaskId": "go-parent-1"}
	res, err := c.SendMessage(ctx, withLineage("lineage", carried))
	task, isTask := res.(*a2a.Task)
	if err != nil || !isTask {
		s.fail("lineage: carried", "send: %v (%T)", err, res)
	} else if got, err := c.GetTask(ctx, &a2a.GetTaskRequest{ID: task.ID}); err != nil {
		s.fail("lineage: carried", "get: %v", err)
	} else {
		// Compare as JSON, so a number decoded as float64 equals the int sent.
		want, _ := json.Marshal(carried)
		have, _ := json.Marshal(got.Metadata[lineageKey])
		var w, h any
		_ = json.Unmarshal(want, &w)
		_ = json.Unmarshal(have, &h)
		if reflect.DeepEqual(w, h) {
			s.ok("lineage: carried is recorded on the task", string(have))
		} else {
			s.fail("lineage: carried is recorded on the task", "got %s, want %s", have, want)
		}
	}

	_, err = c.SendMessage(ctx, withLineage("lineage", map[string]any{"root": "go-job", "depth": 17}))
	const why = "lineage depth 17 is past the cap of 16"
	switch {
	case binding != a2a.TransportProtocolHTTPJSON:
		s.expect("lineage: past the depth cap -> InvalidParams", err, a2a.ErrInvalidParams)
	// Over HTTP+JSON the server answers 400 INVALID_ARGUMENT with no
	// a2a-protocol.org ErrorInfo (it adds one only for A2A's own codes), and
	// a2a-go reads such a body as an internal error, so only the message
	// identifies the refusal here.
	case err == nil:
		s.fail("lineage: past the depth cap is refused", "expected a refusal, got success")
	case !strings.Contains(err.Error(), why):
		s.fail("lineage: past the depth cap is refused", "got %T: %v", err, err)
	default:
		s.ok("lineage: past the depth cap is refused", err.Error())
	}
}
