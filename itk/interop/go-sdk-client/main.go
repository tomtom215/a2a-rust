// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
//
// The official Go SDK's CLIENT, driven against this repository's SERVER.
//
// CI already runs a2a-go as a server (tck.yml, the go-sdk leg), but until this
// program nothing ran a2a-go as a client against a Rust server, and nothing
// ran either direction over gRPC. Every check below is one that the Rust
// server's own tests pass while a Go client — the most common peer a Rust
// coordinator serves — sees something different:
//
//   - a streaming call to an unknown task over JSON-RPC comes back as a
//     plain JSON 200 that a2a-go's SSE reader skips, so the Go caller sees an
//     empty stream and a nil error (audit finding S2). That shape is the one
//     the official a2a-tck requires, so it stays; the JSON-RPC check pins
//     a2a-go's loss and goes red when a2a-go reads the error;
//   - push deliveries carried a token header a2a-go does not read (S7);
//   - an agent card with securityRequirements was unparseable by a2a-go (T1).
//     This SDK now reads a2a-go's bare-array scopes and writes the spec's
//     {"list":[...]} (a2a.proto's StringList, the spec's §8.5 sample, the
//     Python SDK's output). a2a-go v2.5.0 still rejects that, which only
//     a2a-go can fix, so -expect-card-rejected pins the rejection: when a
//     new a2a-go reads the spec shape, this check fails and says to drop it.
//
// Usage: go-sdk-client [-expect-card-rejected] [-expect-security] <agent-base-url> [binding,...]
//
// Exits 0 only when every check passed on every binding the card lists; a
// binding that the caller names but the card omits is a failure, not a skip,
// because a gate that silently narrows is the defect this program exists to
// prevent.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/a2aproject/a2a-go/v2/a2a"
	"github.com/a2aproject/a2a-go/v2/a2aclient"
	"github.com/a2aproject/a2a-go/v2/a2aclient/agentcard"
	grpcv1 "github.com/a2aproject/a2a-go/v2/a2agrpc/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

// The header a2a-go's own push sender writes (a2asrv/push/sender.go:40), and
// therefore the one a Go webhook written against that SDK checks.
const goTokenHeader = "A2A-Notification-Token"

type suite struct {
	passed, failed int
}

func (s *suite) ok(name, detail string) {
	s.passed++
	fmt.Printf("  [ok]   %-44s %s\n", name, detail)
}

func (s *suite) fail(name string, format string, args ...any) {
	s.failed++
	fmt.Printf("  [FAIL] %-44s %s\n", name, fmt.Sprintf(format, args...))
}

// expect records `err` as a pass when it matches `want` under errors.Is.
func (s *suite) expect(name string, err, want error) {
	switch {
	case err == nil:
		s.fail(name, "expected %v, got success", want)
	case !errors.Is(err, want):
		s.fail(name, "expected %v, got %T: %v", want, err, err)
	default:
		s.ok(name, err.Error())
	}
}

func text(t string) *a2a.SendMessageRequest {
	return &a2a.SendMessageRequest{Message: a2a.NewMessage(a2a.MessageRoleUser, a2a.NewTextPart(t))}
}

// The a2a-go error that rejecting the spec's StringList shape produces
// (a2a/auth.go, SecurityRequirementsOptions.UnmarshalJSON into
// SecuritySchemeScopes []string).
const upstreamCardRejection = "a2a.SecuritySchemeScopes"

func main() {
	expectRejected := flag.Bool("expect-card-rejected", false,
		"assert a2a-go still rejects the agent's spec-shaped securityRequirements, and stop")
	expectSecurity := flag.Bool("expect-security", false,
		"require the card to publish apiKey [read write] securityRequirements")
	flag.Parse()
	if flag.NArg() < 1 {
		fmt.Fprintln(os.Stderr, "usage: go-sdk-client [-expect-card-rejected] [-expect-security] <agent-base-url> [binding,...]")
		os.Exit(2)
	}
	base := flag.Arg(0)
	want := []a2a.TransportProtocol{a2a.TransportProtocolJSONRPC, a2a.TransportProtocolHTTPJSON, a2a.TransportProtocolGRPC}
	if flag.NArg() > 1 {
		want = nil
		for _, b := range strings.Split(flag.Arg(1), ",") {
			want = append(want, a2a.TransportProtocol(b))
		}
	}

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	s := &suite{}

	fmt.Printf("=== agent card (%s) ===\n", base)
	card, err := agentcard.DefaultResolver.Resolve(ctx, base)
	if *expectRejected {
		switch {
		case err == nil:
			s.fail("a2a-go rejects spec-shaped scopes (known divergence)",
				"a2a-go now reads {\"list\":[...]}: drop -expect-card-rejected and run the full battery with -expect-security")
		case !strings.Contains(err.Error(), upstreamCardRejection):
			s.fail("a2a-go rejects spec-shaped scopes (known divergence)", "rejected for another reason: %v", err)
		default:
			s.ok("a2a-go rejects spec-shaped scopes (known divergence)", err.Error())
		}
		finish(s)
	}
	if err != nil {
		s.fail("resolve agent card", "%v", err)
		finish(s)
	}
	s.ok("resolve agent card", card.Name)
	if *expectSecurity {
		checkCardSecurity(s, card)
	}

	hook := startWebhook()
	defer hook.close()

	for _, binding := range want {
		fmt.Printf("=== %s ===\n", binding)
		var ifaces []*a2a.AgentInterface
		for _, i := range card.SupportedInterfaces {
			if i.ProtocolBinding == binding {
				ifaces = append(ifaces, i)
			}
		}
		if len(ifaces) == 0 {
			s.fail("binding listed in card", "%s requested but the card does not list it", binding)
			continue
		}
		client, err := a2aclient.NewFromEndpoints(ctx, ifaces,
			grpcv1.WithGRPCTransport(grpc.WithTransportCredentials(insecure.NewCredentials())))
		if err != nil {
			s.fail("create client", "%v", err)
			continue
		}
		run(ctx, s, client, binding, hook)
		_ = client.Destroy()
	}
	finish(s)
}

func finish(s *suite) {
	fmt.Printf("\n%d passed, %d failed\n", s.passed, s.failed)
	if s.failed > 0 || s.passed == 0 {
		os.Exit(1)
	}
	os.Exit(0)
}

// checkCardSecurity runs under -expect-security, once a2a-go can read the
// spec's shape.
func checkCardSecurity(s *suite, card *a2a.AgentCard) {
	if len(card.SecurityRequirements) == 0 {
		s.fail("card securityRequirements", "none published; start the agent with A2A_INTEROP_CARD=1")
		return
	}
	scopes := card.SecurityRequirements[0]["apiKey"]
	if len(scopes) != 2 || scopes[0] != "read" || scopes[1] != "write" {
		s.fail("card securityRequirements", "apiKey scopes = %v, want [read write]", scopes)
		return
	}
	s.ok("card securityRequirements", fmt.Sprint(card.SecurityRequirements))
}

func run(ctx context.Context, s *suite, c *a2aclient.Client, binding a2a.TransportProtocol, hook *webhook) {
	// ── Unary lifecycle ───────────────────────────────────────────────────
	res, err := c.SendMessage(ctx, text("hello"))
	task, isTask := res.(*a2a.Task)
	switch {
	case err != nil:
		s.fail("SendMessage", "%v", err)
		return
	case !isTask:
		s.fail("SendMessage", "expected a Task, got %T", res)
		return
	case task.Status.State != a2a.TaskStateCompleted:
		s.fail("SendMessage", "state %s, want completed", task.Status.State)
	default:
		s.ok("SendMessage", string(task.ID))
	}

	if got, err := c.GetTask(ctx, &a2a.GetTaskRequest{ID: task.ID}); err != nil {
		s.fail("GetTask", "%v", err)
	} else if got.ID != task.ID {
		s.fail("GetTask", "id %s, want %s", got.ID, task.ID)
	} else {
		s.ok("GetTask", string(got.Status.State))
	}

	if list, err := c.ListTasks(ctx, &a2a.ListTasksRequest{PageSize: 50}); err != nil {
		s.fail("ListTasks", "%v", err)
	} else {
		s.ok("ListTasks", fmt.Sprintf("%d tasks", len(list.Tasks)))
	}

	_, err = c.CancelTask(ctx, &a2a.CancelTaskRequest{ID: task.ID})
	s.expect("CancelTask(completed) -> NotCancelable", err, a2a.ErrTaskNotCancelable)

	// ── Push-notification config CRUD ─────────────────────────────────────
	cfg := &a2a.PushConfig{TaskID: task.ID, ID: "cfg-1", URL: hook.url, Token: "tok-crud"}
	if _, err := c.CreateTaskPushConfig(ctx, cfg); err != nil {
		s.fail("CreateTaskPushConfig", "%v", err)
	} else {
		s.ok("CreateTaskPushConfig", cfg.ID)
	}
	if _, err := c.GetTaskPushConfig(ctx, &a2a.GetTaskPushConfigRequest{TaskID: task.ID, ID: cfg.ID}); err != nil {
		s.fail("GetTaskPushConfig", "%v", err)
	} else {
		s.ok("GetTaskPushConfig", cfg.ID)
	}
	if list, err := c.ListTaskPushConfigs(ctx, &a2a.ListTaskPushConfigRequest{TaskID: task.ID}); err != nil {
		s.fail("ListTaskPushConfigs", "%v", err)
	} else if len(list) != 1 {
		s.fail("ListTaskPushConfigs", "%d configs, want 1", len(list))
	} else {
		s.ok("ListTaskPushConfigs", "1 config")
	}
	if err := c.DeleteTaskPushConfig(ctx, &a2a.DeleteTaskPushConfigRequest{TaskID: task.ID, ID: cfg.ID}); err != nil {
		s.fail("DeleteTaskPushConfig", "%v", err)
	} else {
		s.ok("DeleteTaskPushConfig", cfg.ID)
	}

	// ── Errors a Go caller must be able to tell apart ─────────────────────
	_, err = c.GetTask(ctx, &a2a.GetTaskRequest{ID: "does-not-exist"})
	s.expect("GetTask(missing) -> TaskNotFound", err, a2a.ErrTaskNotFound)
	_, err = c.CancelTask(ctx, &a2a.CancelTaskRequest{ID: "does-not-exist"})
	s.expect("CancelTask(missing) -> TaskNotFound", err, a2a.ErrTaskNotFound)

	// S2: the streaming methods' pre-stream errors. Draining to the end and
	// taking the first error is what a caller does.
	orphan := a2a.NewMessage(a2a.MessageRoleUser, a2a.NewTextPart("continue"))
	orphan.TaskID = "does-not-exist"
	streamErr := firstErrOrEmpty(c.SendStreamingMessage(ctx, &a2a.SendMessageRequest{Message: orphan}))
	subscribeErr := firstErrOrEmpty(c.SubscribeToTask(ctx, &a2a.SubscribeToTaskRequest{ID: "does-not-exist"}))
	if binding == a2a.TransportProtocolJSONRPC {
		// Over JSON-RPC this server answers a pre-stream error as a plain
		// JSON-RPC error body, the shape the official a2a-tck requires
		// (STREAM-SUB-003/004). a2a-go v2.5.0's client reads a streaming
		// answer only as SSE, so it sees an empty stream and no error. That is
		// a2a-go's to fix; pinned here so the day it is fixed this goes red
		// and the strict expectation is restored.
		s.expectLostByGo("SendStreamingMessage(missing task) [a2a-go drops it]", streamErr)
		s.expectLostByGo("SubscribeToTask(missing) [a2a-go drops it]", subscribeErr)
	} else {
		s.expect("SendStreamingMessage(missing task) -> TaskNotFound", streamErr.err, a2a.ErrTaskNotFound)
		s.expect("SubscribeToTask(missing) -> TaskNotFound", subscribeErr.err, a2a.ErrTaskNotFound)
	}

	// ── Streaming ─────────────────────────────────────────────────────────
	events, last, err := drain(c.SendStreamingMessage(ctx, text("stream me")))
	switch {
	case err != nil:
		s.fail("SendStreamingMessage", "%v", err)
	case !terminal(last, a2a.TaskStateCompleted):
		s.fail("SendStreamingMessage", "%d events, last %T is not a completed status", events, last)
	default:
		s.ok("SendStreamingMessage", fmt.Sprintf("%d events, ends completed", events))
	}

	runInFlight(ctx, s, c)
	runPushDelivery(ctx, s, c, hook)

	if card, err := c.GetExtendedAgentCard(ctx, &a2a.GetExtendedAgentCardRequest{}); err != nil {
		s.fail("GetExtendedAgentCard", "%v", err)
	} else {
		s.ok("GetExtendedAgentCard", card.Name)
	}
}

// runInFlight subscribes to, then cancels, tasks that are still running.
func runInFlight(ctx context.Context, s *suite, c *a2aclient.Client) {
	id, err := startSlow(ctx, c, "slow: subscribe")
	if err != nil {
		s.fail("SubscribeToTask(in-flight)", "starting slow task: %v", err)
	} else {
		events, last, err := drain(c.SubscribeToTask(ctx, &a2a.SubscribeToTaskRequest{ID: id}))
		switch {
		case err != nil:
			s.fail("SubscribeToTask(in-flight)", "%v", err)
		case !terminal(last, a2a.TaskStateCompleted):
			s.fail("SubscribeToTask(in-flight)", "%d events, last %T is not a completed status", events, last)
		default:
			s.ok("SubscribeToTask(in-flight)", fmt.Sprintf("%d events, ends completed", events))
		}
	}

	id, err = startSlow(ctx, c, "slow: cancel")
	if err != nil {
		s.fail("CancelTask(in-flight)", "starting slow task: %v", err)
		return
	}
	task, err := c.CancelTask(ctx, &a2a.CancelTaskRequest{ID: id})
	switch {
	case err != nil:
		s.fail("CancelTask(in-flight)", "%v", err)
	case task.Status.State != a2a.TaskStateCanceled:
		s.fail("CancelTask(in-flight)", "state %s, want canceled", task.Status.State)
	default:
		s.ok("CancelTask(in-flight)", "canceled")
	}
}

// runPushDelivery registers a config inline with the send, so delivery cannot
// race the registration, and checks what a Go webhook would see.
func runPushDelivery(ctx context.Context, s *suite, c *a2aclient.Client, hook *webhook) {
	token := fmt.Sprintf("tok-%d", time.Now().UnixNano())
	req := text("push me")
	req.Config = &a2a.SendMessageConfig{PushConfig: &a2a.PushConfig{URL: hook.url, Token: token}}
	res, err := c.SendMessage(ctx, req)
	task, isTask := res.(*a2a.Task)
	if err != nil || !isTask {
		s.fail("push delivery", "send failed: %v (%T)", err, res)
		return
	}
	got, ok := hook.await(string(task.ID), 10*time.Second)
	switch {
	case !ok:
		s.fail("push delivery", "no notification for task %s within 10s", task.ID)
	case got.header.Get(goTokenHeader) != token:
		s.fail("push delivery token header", "%s = %q, want %q (headers: %v)",
			goTokenHeader, got.header.Get(goTokenHeader), token, got.header)
	default:
		s.ok("push delivery token header", goTokenHeader)
	}
}

func startSlow(ctx context.Context, c *a2aclient.Client, prompt string) (a2a.TaskID, error) {
	sctx, cancel := context.WithCancel(ctx)
	defer cancel()
	for ev, err := range c.SendStreamingMessage(sctx, text(prompt)) {
		if err != nil {
			return "", err
		}
		if id := ev.TaskInfo().TaskID; id != "" {
			return id, nil
		}
	}
	return "", errors.New("stream ended before naming a task")
}

// streamOutcome is what draining a stream to its first error saw.
type streamOutcome struct {
	events int
	err    error
}

func firstErrOrEmpty(seq func(func(a2a.Event, error) bool)) streamOutcome {
	var out streamOutcome
	for _, err := range seq {
		if err != nil {
			out.err = err
			return out
		}
		out.events++
	}
	return out
}

// expectLostByGo passes only on a2a-go's known behaviour for a JSON-RPC
// pre-stream error: no events and no error. A TaskNotFound means a2a-go now
// reads the error, which is the fix this pin waits for; anything else is a
// real failure.
func (s *suite) expectLostByGo(name string, got streamOutcome) {
	switch {
	case got.events == 0 && got.err == nil:
		s.ok(name, "empty stream, nil error (known a2a-go divergence)")
	case errors.Is(got.err, a2a.ErrTaskNotFound):
		s.fail(name, "a2a-go now reports TaskNotFound: restore the strict expectation for JSON-RPC")
	default:
		s.fail(name, "events=%d err=%v", got.events, got.err)
	}
}

func drain(seq func(func(a2a.Event, error) bool)) (int, a2a.Event, error) {
	n := 0
	var last a2a.Event
	for ev, err := range seq {
		if err != nil {
			return n, last, err
		}
		n++
		last = ev
	}
	return n, last, nil
}

func terminal(ev a2a.Event, want a2a.TaskState) bool {
	switch e := ev.(type) {
	case *a2a.TaskStatusUpdateEvent:
		return e.Status.State == want
	case *a2a.Task:
		return e.Status.State == want
	}
	return false
}

// ── Webhook ──────────────────────────────────────────────────────────────

type delivery struct {
	header http.Header
	body   []byte
}

type webhook struct {
	url    string
	server *http.Server
	mu     sync.Mutex
	byTask map[string][]delivery
	notify chan struct{}
}

func startWebhook() *webhook {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		fmt.Fprintf(os.Stderr, "webhook listen: %v\n", err)
		os.Exit(2)
	}
	h := &webhook{
		url:    "http://" + listener.Addr().String() + "/hook",
		byTask: map[string][]delivery{},
		notify: make(chan struct{}, 1),
	}
	h.server = &http.Server{Handler: http.HandlerFunc(h.serve), ReadHeaderTimeout: 5 * time.Second}
	go func() { _ = h.server.Serve(listener) }()
	return h
}

func (h *webhook) serve(w http.ResponseWriter, r *http.Request) {
	body, _ := io.ReadAll(io.LimitReader(r.Body, 1<<20))
	id := taskIDOf(body)
	h.mu.Lock()
	h.byTask[id] = append(h.byTask[id], delivery{header: r.Header.Clone(), body: body})
	h.mu.Unlock()
	select {
	case h.notify <- struct{}{}:
	default:
	}
	w.WriteHeader(http.StatusOK)
}

// taskIDOf finds the task id in a push body without committing to one of the
// StreamResponse arms: the id is `task.id`, or `taskId` on an update event.
func taskIDOf(body []byte) string {
	var v map[string]map[string]any
	if json.Unmarshal(body, &v) != nil {
		return ""
	}
	for arm, inner := range v {
		key := "taskId"
		if arm == "task" {
			key = "id"
		}
		if id, ok := inner[key].(string); ok {
			return id
		}
	}
	return ""
}

func (h *webhook) await(taskID string, within time.Duration) (delivery, bool) {
	deadline := time.After(within)
	for {
		h.mu.Lock()
		got := h.byTask[taskID]
		h.mu.Unlock()
		if len(got) > 0 {
			return got[0], true
		}
		select {
		case <-h.notify:
		case <-deadline:
			return delivery{}, false
		}
	}
}

func (h *webhook) close() { _ = h.server.Close() }
