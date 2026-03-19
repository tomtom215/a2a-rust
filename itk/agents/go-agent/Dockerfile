FROM golang:1.22-alpine AS build
WORKDIR /app
COPY go.mod go.sum* ./
RUN go mod download 2>/dev/null || true
COPY . .
RUN go build -o /agent .

FROM alpine:3.19
COPY --from=build /agent /agent
EXPOSE 9102
CMD ["/agent", "--port", "9102"]
