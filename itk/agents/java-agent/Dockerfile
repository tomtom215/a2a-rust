FROM maven:3.9-eclipse-temurin-17-alpine@sha256:b7c8d7298dbabbe750441736cd99a1d120a58083d5af8d2bd75a1624cfdd8f3a AS build
WORKDIR /app
COPY pom.xml .
RUN mvn dependency:go-offline -q
COPY src src
RUN mvn package -q -DskipTests

FROM eclipse-temurin:17-jre-alpine@sha256:3c472129dc75a8d1d7a3f2df5b2093a8077e4493deff046754d4764d0371de63
COPY --from=build /app/target/*.jar /app.jar
COPY --from=build /app/target/dependency/* /libs/
EXPOSE 9103
CMD ["java", "-cp", "/app.jar:/libs/*", "com.example.EchoAgent", "--port", "9103"]
