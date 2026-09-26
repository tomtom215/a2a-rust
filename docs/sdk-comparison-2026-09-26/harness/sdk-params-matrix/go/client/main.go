// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

package main

import (
	"context"
	"fmt"

	"github.com/a2aproject/a2a-go/v2/a2a"
	"github.com/a2aproject/a2a-go/v2/a2aclient"
	"github.com/a2aproject/a2a-go/v2/a2aclient/agentcard"
)

func main() {
	ctx := context.Background()
	c, err := agentcard.DefaultResolver.Resolve(ctx, "http://127.0.0.1:7692")
	if err != nil { panic(err) }
	cl, err := a2aclient.NewFromCard(ctx, c)
	if err != nil { panic(err) }
	ext, err := cl.GetExtendedAgentCard(ctx, &a2a.GetExtendedAgentCardRequest{})
	if err != nil { panic(err) }
	fmt.Println("got card:", ext.Name)
}
