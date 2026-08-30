// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
//
// A2A echo agent built on the OFFICIAL Java SDK (org.a2aproject.sdk),
// served by the reference Quarkus JSON-RPC + REST transports. Running our
// TCK against it validates this Rust SDK's wire expectations against the
// reference Java implementation.
package net.tomtomtech.a2a.itk;

import java.util.List;

import jakarta.enterprise.context.ApplicationScoped;
import jakarta.enterprise.inject.Produces;

import org.a2aproject.sdk.server.PublicAgentCard;
import org.a2aproject.sdk.server.ServerCallContext;
import org.a2aproject.sdk.server.auth.TaskAuthorizationProvider;
import org.a2aproject.sdk.server.auth.TaskOperation;
import org.a2aproject.sdk.server.agentexecution.AgentExecutor;
import org.a2aproject.sdk.server.agentexecution.RequestContext;
import org.a2aproject.sdk.server.tasks.AgentEmitter;
import org.a2aproject.sdk.spec.A2AError;
import org.a2aproject.sdk.spec.AgentCapabilities;
import org.a2aproject.sdk.spec.AgentCard;
import org.a2aproject.sdk.spec.AgentInterface;
import org.a2aproject.sdk.spec.AgentSkill;
import org.a2aproject.sdk.spec.TaskNotCancelableError;
import org.a2aproject.sdk.spec.TaskState;
import org.a2aproject.sdk.spec.TextPart;
import org.a2aproject.sdk.spec.TransportProtocol;

@ApplicationScoped
public class EchoAgent {

    private static final String AGENT_URL = "http://127.0.0.1:9113";

    @Produces
    @PublicAgentCard
    public AgentCard agentCard() {
        return AgentCard.builder()
                .name("official-java-echo")
                .description("Echo agent built on the official a2a-java SDK")
                .version("1.0.0")
                .supportedInterfaces(List.of(
                        new AgentInterface(TransportProtocol.JSONRPC.asString(), AGENT_URL),
                        new AgentInterface(TransportProtocol.HTTP_JSON.asString(), AGENT_URL)))
                .capabilities(AgentCapabilities.builder()
                        .streaming(true)
                        .pushNotifications(true)
                        .build())
                .defaultInputModes(List.of("text/plain"))
                .defaultOutputModes(List.of("text/plain"))
                .skills(List.of(AgentSkill.builder()
                        .id("echo")
                        .name("Echo")
                        .description("Echoes back the input text")
                        .tags(List.of("echo", "test"))
                        .build()))
                .build();
    }

    @Produces
    public AgentExecutor agentExecutor() {
        return new EchoExecutor();
    }

    /// Every caller may do everything, stated as policy rather than assumed.
    ///
    /// `a2a-java` 1.3.0.Final enforces a fail-closed default for task
    /// authorization. `DefaultRequestHandler.enforceRead` reads, in bytecode:
    /// with no `TaskAuthorizationProvider` bean and `authorizationRequired`
    /// set, every task read throws `TaskNotFoundError` — a denied read is
    /// reported as "not found" rather than leaking that the task exists.
    /// Without this bean the agent still starts, still answers
    /// `SendMessage`, and then fails every check that reads a task back,
    /// which reads like a broken task store and is not one.
    ///
    /// The other way to close it is `a2a.authorization.required=false`, which
    /// switches the check off. This grants instead, so the authorization path
    /// still runs and the ITK exercises it. For this agent that is the honest
    /// policy and not a bypass: the card advertises no security schemes, so
    /// "unauthenticated callers may do everything" is what it actually
    /// implements. An agent that meant to restrict anything would deny here.
    @Produces
    @ApplicationScoped
    public TaskAuthorizationProvider taskAuthorizationProvider() {
        return new PermitAllTaskAuthorization();
    }

    private static final class PermitAllTaskAuthorization implements TaskAuthorizationProvider {
        @Override
        public boolean checkRead(ServerCallContext context, String taskId, TaskOperation operation) {
            return true;
        }

        @Override
        public boolean checkWrite(ServerCallContext context, String taskId, TaskOperation operation) {
            return true;
        }

        @Override
        public boolean checkCreate(ServerCallContext context, TaskOperation operation) {
            return true;
        }

        /// `true`, so `recordOwnership` is never called.
        ///
        /// `DefaultRequestHandler.recordOwnershipIfNeeded` calls this purely as
        /// an idempotence guard: it records ownership only when this answers
        /// `false`. Nothing is recorded here because nothing is enforced, so
        /// there is never an outstanding recording to make. Answering `false`
        /// would also work and would call a no-op on every request.
        @Override
        public boolean isTaskRecorded(String taskId) {
            return true;
        }

        @Override
        public void recordOwnership(
                ServerCallContext context, String taskId, TaskOperation operation) {
            // No ownership model: see isTaskRecorded.
        }
    }

    private static final class EchoExecutor implements AgentExecutor {
        @Override
        public void execute(RequestContext context, AgentEmitter emitter) throws A2AError {
            if (context.getTask() == null) {
                emitter.submit();
            }
            emitter.startWork();
            String text = context.getUserInput();
            emitter.addArtifact(List.of(new TextPart("Echo: " + text)));
            emitter.complete();
        }

        @Override
        public void cancel(RequestContext context, AgentEmitter emitter) throws A2AError {
            var task = context.getTask();
            if (task != null && task.status() != null) {
                TaskState state = task.status().state();
                if (state == TaskState.TASK_STATE_CANCELED
                        || state == TaskState.TASK_STATE_COMPLETED
                        || state == TaskState.TASK_STATE_FAILED) {
                    throw new TaskNotCancelableError();
                }
            }
            emitter.cancel();
        }
    }
}
