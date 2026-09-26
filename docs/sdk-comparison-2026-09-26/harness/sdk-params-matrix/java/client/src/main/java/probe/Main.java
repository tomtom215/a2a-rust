// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

package probe;
import org.a2aproject.sdk.client.Client;
import org.a2aproject.sdk.client.http.A2ACardResolver;
import org.a2aproject.sdk.client.transport.jsonrpc.JSONRPCTransport;
import org.a2aproject.sdk.client.transport.jsonrpc.JSONRPCTransportConfigBuilder;
import org.a2aproject.sdk.spec.AgentCard;
public class Main {
    public static void main(String[] a) throws Exception {
        AgentCard card = A2ACardResolver.builder().baseUrl(a[0]).build().getAgentCard();
        Client c = Client.builder(card).withTransport(JSONRPCTransport.class, new JSONRPCTransportConfigBuilder()).build();
        System.out.println("got card: " + c.getExtendedAgentCard().name());
        c.close();
    }
}
