// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

package probe;

import java.util.List;
import jakarta.enterprise.context.ApplicationScoped;
import jakarta.enterprise.inject.Produces;
import org.a2aproject.sdk.server.ExtendedAgentCard;
import org.a2aproject.sdk.server.PublicAgentCard;
import org.a2aproject.sdk.server.agentexecution.AgentExecutor;
import org.a2aproject.sdk.server.agentexecution.RequestContext;
import org.a2aproject.sdk.server.tasks.AgentEmitter;
import org.a2aproject.sdk.spec.*;

@ApplicationScoped
public class Producers {
    static AgentCard card(String name) {
        return AgentCard.builder().name(name).description("d").version("1.0.0")
            .supportedInterfaces(List.of(new AgentInterface("JSONRPC", "http://127.0.0.1:7651")))
            .capabilities(AgentCapabilities.builder().extendedAgentCard(true).build())
            .defaultInputModes(List.of("text/plain")).defaultOutputModes(List.of("text/plain"))
            .skills(List.of(AgentSkill.builder().id("s").name("s").description("s").tags(List.of("t")).build()))
            .build();
    }
    @Produces @PublicAgentCard public AgentCard pub() { return card("java-public"); }
    @Produces @ExtendedAgentCard public AgentCard ext() { return card("java-EXTENDED"); }
    @Produces public AgentExecutor exec() {
        return new AgentExecutor() {
            public void execute(RequestContext c, AgentEmitter e) throws A2AError { e.sendMessage("hi"); }
            public void cancel(RequestContext c, AgentEmitter e) throws A2AError { throw new UnsupportedOperationError(); }
        };
    }
}
