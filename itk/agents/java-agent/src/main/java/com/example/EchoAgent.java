// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

package com.example;

import com.google.gson.*;
import com.sun.net.httpserver.HttpServer;
import com.sun.net.httpserver.HttpExchange;

import java.io.*;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/**
 * A2A ITK — Java Echo Agent
 *
 * A minimal A2A agent using Java's built-in HTTP server that echoes incoming messages.
 * Used for cross-language interoperability testing with the Rust A2A SDK.
 */
public class EchoAgent {
    private static final Gson gson = new GsonBuilder().create();
    private static final Map<String, JsonObject> tasks = new ConcurrentHashMap<>();
    private static final Map<String, JsonObject> pushConfigs = new ConcurrentHashMap<>();
    private static int port = 9103;

    public static void main(String[] args) throws Exception {
        for (int i = 0; i < args.length; i++) {
            if ("--port".equals(args[i]) && i + 1 < args.length) {
                port = Integer.parseInt(args[++i]);
            }
        }

        HttpServer server = HttpServer.create(new InetSocketAddress("0.0.0.0", port), 0);

        server.createContext("/.well-known/agent.json", EchoAgent::handleAgentCard);
        server.createContext("/", EchoAgent::handleRequest);

        server.start();
        System.out.printf("[ITK] Java Echo Agent listening on port %d%n", port);
    }

    private static void handleAgentCard(HttpExchange exchange) throws IOException {
        JsonObject card = new JsonObject();
        card.addProperty("name", "ITK Java Echo Agent");
        card.addProperty("description", "A2A ITK echo agent implemented in Java");
        card.addProperty("url", "http://0.0.0.0:" + port);
        card.addProperty("version", "1.0.0");

        JsonObject capabilities = new JsonObject();
        capabilities.addProperty("streaming", true);
        capabilities.addProperty("pushNotifications", true);
        card.add("capabilities", capabilities);

        JsonArray inputModes = new JsonArray();
        inputModes.add("text");
        card.add("defaultInputModes", inputModes);

        JsonArray outputModes = new JsonArray();
        outputModes.add("text");
        card.add("defaultOutputModes", outputModes);

        JsonArray skills = new JsonArray();
        JsonObject skill = new JsonObject();
        skill.addProperty("id", "echo");
        skill.addProperty("name", "Echo");
        skill.addProperty("description", "Echoes the input message back");
        JsonArray tags = new JsonArray();
        tags.add("echo");
        tags.add("test");
        skill.add("tags", tags);
        skills.add(skill);
        card.add("skills", skills);

        sendJson(exchange, 200, card);
    }

    private static void handleRequest(HttpExchange exchange) throws IOException {
        String method = exchange.getRequestMethod();
        String path = exchange.getRequestURI().getPath();

        if ("GET".equals(method)) {
            handleGet(exchange, path);
            return;
        }

        if (!"POST".equals(method)) {
            sendJson(exchange, 405, errorJson(-32600, "Method not allowed"));
            return;
        }

        String body = new String(exchange.getRequestBody().readAllBytes(), StandardCharsets.UTF_8);
        JsonObject json;
        try {
            json = JsonParser.parseString(body).getAsJsonObject();
        } catch (Exception e) {
            sendJson(exchange, 400, errorJson(-32700, "Parse error"));
            return;
        }

        // Check if JSON-RPC
        if (json.has("jsonrpc")) {
            handleJsonRpc(exchange, json);
        } else {
            handleRest(exchange, path, json);
        }
    }

    private static void handleGet(HttpExchange exchange, String path) throws IOException {
        if ("/tasks".equals(path)) {
            JsonObject result = new JsonObject();
            JsonArray arr = new JsonArray();
            tasks.values().forEach(arr::add);
            result.add("tasks", arr);
            sendJson(exchange, 200, result);
        } else if (path.matches("/tasks/[^/]+/pushNotificationConfig/[^/]+")) {
            // GET /tasks/{taskId}/pushNotificationConfig/{configId}
            String[] parts = path.split("/");
            String taskId = parts[2];
            String configId = parts[4];
            String key = taskId + ":" + configId;
            JsonObject cfg = pushConfigs.get(key);
            if (cfg != null) {
                sendJson(exchange, 200, cfg);
            } else {
                sendJson(exchange, 404, errorJson(-32001, "Config not found"));
            }
        } else if (path.matches("/tasks/[^/]+/pushNotificationConfig")) {
            // GET /tasks/{taskId}/pushNotificationConfig — list
            String taskId = path.split("/")[2];
            JsonArray configs = new JsonArray();
            pushConfigs.forEach((k, v) -> {
                if (k.startsWith(taskId + ":")) configs.add(v);
            });
            sendJson(exchange, 200, configs);
        } else if (path.startsWith("/tasks/")) {
            String taskId = path.substring("/tasks/".length());
            JsonObject task = tasks.get(taskId);
            if (task != null) {
                sendJson(exchange, 200, task);
            } else {
                sendJson(exchange, 404, errorJson(-32001, "Task not found"));
            }
        } else {
            sendJson(exchange, 404, errorJson(-32601, "Not found"));
        }
    }

    private static void handleJsonRpc(HttpExchange exchange, JsonObject req) throws IOException {
        JsonElement id = req.get("id");
        String method = req.has("method") ? req.get("method").getAsString() : "";
        JsonObject params = req.has("params") ? req.getAsJsonObject("params") : new JsonObject();

        JsonObject result;

        switch (method) {
            case "message/send":
            case "message/stream":
                if (!params.has("message")) {
                    sendJsonRpc(exchange, id, null, errorJson(-32602, "Invalid params: missing 'message' field"));
                    break;
                }
                result = processMessage(params);
                sendJsonRpc(exchange, id, result, null);
                break;

            case "tasks/get": {
                String taskId = params.has("id") ? params.get("id").getAsString() : "";
                JsonObject task = tasks.get(taskId);
                if (task != null) {
                    sendJsonRpc(exchange, id, task, null);
                } else {
                    sendJsonRpc(exchange, id, null, errorJson(-32001, "Task not found"));
                }
                break;
            }

            case "tasks/list": {
                JsonObject listResult = new JsonObject();
                JsonArray arr = new JsonArray();
                tasks.values().forEach(arr::add);
                listResult.add("tasks", arr);
                sendJsonRpc(exchange, id, listResult, null);
                break;
            }

            case "tasks/cancel": {
                String taskId = params.has("id") ? params.get("id").getAsString() : "";
                JsonObject task = tasks.get(taskId);
                if (task != null) {
                    JsonObject status = new JsonObject();
                    status.addProperty("state", "canceled");
                    task.add("status", status);
                    sendJsonRpc(exchange, id, task, null);
                } else {
                    sendJsonRpc(exchange, id, null, errorJson(-32001, "Task not found"));
                }
                break;
            }

            case "tasks/pushNotificationConfig/set": {
                String cfgId = params.has("id") ? params.get("id").getAsString() : UUID.randomUUID().toString();
                params.addProperty("id", cfgId);
                String taskId = params.has("taskId") ? params.get("taskId").getAsString() : "";
                pushConfigs.put(taskId + ":" + cfgId, params);
                sendJsonRpc(exchange, id, params, null);
                break;
            }

            case "tasks/pushNotificationConfig/get": {
                String taskId = params.has("taskId") ? params.get("taskId").getAsString() : "";
                String cfgId = params.has("id") ? params.get("id").getAsString() : "";
                JsonObject cfg = pushConfigs.get(taskId + ":" + cfgId);
                if (cfg != null) {
                    sendJsonRpc(exchange, id, cfg, null);
                } else {
                    sendJsonRpc(exchange, id, null, errorJson(-32001, "Config not found"));
                }
                break;
            }

            case "tasks/pushNotificationConfig/list": {
                String taskId = params.has("taskId") ? params.get("taskId").getAsString() : "";
                JsonArray configs = new JsonArray();
                pushConfigs.forEach((k, v) -> {
                    if (k.startsWith(taskId + ":")) configs.add(v);
                });
                sendJsonRpc(exchange, id, configs, null);
                break;
            }

            case "tasks/pushNotificationConfig/delete": {
                String taskId = params.has("taskId") ? params.get("taskId").getAsString() : "";
                String cfgId = params.has("id") ? params.get("id").getAsString() : "";
                pushConfigs.remove(taskId + ":" + cfgId);
                sendJsonRpc(exchange, id, new JsonObject(), null);
                break;
            }

            default:
                sendJsonRpc(exchange, id, null, errorJson(-32601, "Method not found: " + method));
        }
    }

    private static void handleRest(HttpExchange exchange, String path, JsonObject body) throws IOException {
        if ("/message/send".equals(path) || "/message/stream".equals(path)) {
            sendJson(exchange, 200, processMessage(body));
        } else if (path.matches("/tasks/[^/]+/cancel")) {
            // POST /tasks/{taskId}/cancel
            String taskId = path.split("/")[2];
            JsonObject task = tasks.get(taskId);
            if (task != null) {
                JsonObject status = new JsonObject();
                status.addProperty("state", "canceled");
                task.add("status", status);
                sendJson(exchange, 200, task);
            } else {
                sendJson(exchange, 404, errorJson(-32001, "Task not found"));
            }
        } else if (path.matches("/tasks/[^/]+/pushNotificationConfig/[^/]+/delete")) {
            // POST /tasks/{taskId}/pushNotificationConfig/{configId}/delete
            String[] parts = path.split("/");
            String taskId = parts[2];
            String configId = parts[4];
            pushConfigs.remove(taskId + ":" + configId);
            sendJson(exchange, 200, new JsonObject());
        } else if (path.matches("/tasks/[^/]+/pushNotificationConfig")) {
            // POST /tasks/{taskId}/pushNotificationConfig — create
            String taskId = path.split("/")[2];
            String cfgId = body.has("id") ? body.get("id").getAsString() : UUID.randomUUID().toString();
            body.addProperty("id", cfgId);
            body.addProperty("taskId", taskId);
            pushConfigs.put(taskId + ":" + cfgId, body);
            sendJson(exchange, 200, body);
        } else {
            sendJson(exchange, 404, errorJson(-32601, "Not found"));
        }
    }

    private static JsonObject processMessage(JsonObject params) {
        String text = "";
        if (params.has("message")) {
            JsonObject msg = params.getAsJsonObject("message");
            if (msg.has("parts")) {
                for (JsonElement part : msg.getAsJsonArray("parts")) {
                    JsonObject p = part.getAsJsonObject();
                    if ("text".equals(p.has("type") ? p.get("type").getAsString() : "")) {
                        text += p.has("text") ? p.get("text").getAsString() : "";
                    }
                }
            }
        }

        String taskId = UUID.randomUUID().toString();
        String contextId = params.has("contextId") ? params.get("contextId").getAsString() : UUID.randomUUID().toString();

        JsonObject task = new JsonObject();
        task.addProperty("id", taskId);
        task.addProperty("contextId", contextId);

        JsonObject status = new JsonObject();
        status.addProperty("state", "completed");
        task.add("status", status);

        JsonArray artifacts = new JsonArray();
        JsonObject artifact = new JsonObject();
        artifact.addProperty("artifactId", UUID.randomUUID().toString());
        JsonArray parts = new JsonArray();
        JsonObject echoPart = new JsonObject();
        echoPart.addProperty("type", "text");
        echoPart.addProperty("text", "[Java Echo] " + text);
        parts.add(echoPart);
        artifact.add("parts", parts);
        artifacts.add(artifact);
        task.add("artifacts", artifacts);

        if (params.has("message")) {
            JsonArray history = new JsonArray();
            history.add(params.getAsJsonObject("message"));
            task.add("history", history);
        }

        tasks.put(taskId, task);
        return task;
    }

    private static JsonObject errorJson(int code, String message) {
        JsonObject err = new JsonObject();
        err.addProperty("code", code);
        err.addProperty("message", message);
        return err;
    }

    private static void sendJson(HttpExchange exchange, int statusCode, JsonElement body) throws IOException {
        byte[] bytes = gson.toJson(body).getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().set("Content-Type", "application/json");
        exchange.sendResponseHeaders(statusCode, bytes.length);
        exchange.getResponseBody().write(bytes);
        exchange.getResponseBody().close();
    }

    private static void sendJsonRpc(HttpExchange exchange, JsonElement id, JsonElement result, JsonObject error) throws IOException {
        JsonObject resp = new JsonObject();
        resp.addProperty("jsonrpc", "2.0");
        resp.add("id", id);
        if (error != null) {
            resp.add("error", error);
        } else {
            resp.add("result", result);
        }
        sendJson(exchange, 200, resp);
    }
}
