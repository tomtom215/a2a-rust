<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Push notifications

Part of [Upgrading from 0.14 to 0.15](../0.14-to-0.15.md). Each section names what breaks and what to change; the [upgrade guide](../../upgrading.md) has the policy and how to check your own code.

## Push configs the server cannot honour are refused at creation

A push config with an empty or scheme-less `url`, or with an authentication
scheme other than Bearer or Basic, answers `InvalidParams`. The second used to
be stored and then delivered with no credentials at all.

## Push needs a push sender

Spec §3.3.4: when push is not supported, every push-config operation MUST
answer `PushNotificationNotSupported`. A handler with no push sender now
answers that to Get, List and Delete as well as Create; they used to run
against the store. And `build()` refuses an agent card that declares
`capabilities.pushNotifications` with no sender, since every push-config
operation would refuse what the card advertises. Wire one with
`.with_push_sender(HttpPushSender::new())`, or declare `pushNotifications`
false.

## A push config the sender cannot authenticate is not delivered

Spec §13.2: agents MUST send the credentials `authentication` specifies.
`HttpPushSender::send` used to deliver with no `Authorization` header when a
stored config named a scheme other than `Bearer` or `Basic`, or no
credentials; it now refuses the delivery with `InvalidParams`. Creation
already refused such configs, so only one stored another way is affected.

## Push webhook timeouts: 10 s per request, 30 s per delivery

`HttpPushSender`'s request timeout goes from 30 s to 10 s and
`HandlerLimits::push_delivery_timeout` from 5 s to 30 s, both inside the
10-30 s that spec §4.3.3 recommends. The old pair let one attempt of the
sender's three run; now all three start and two always finish. A webhook
that needs more than 10 s per request needs `HttpPushSender::with_timeout`.
Because the delivery bound now equals `push_delivery_budget`, an event's
deliveries run all at once instead of 17 at a time; to bound how many are in
flight, raise `push_delivery_budget` or lower the delivery bound.
