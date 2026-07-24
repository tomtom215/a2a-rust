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
