FROM maven:3.9-eclipse-temurin-17-alpine AS build
WORKDIR /app
COPY pom.xml .
RUN mvn dependency:go-offline -q
COPY src src
RUN mvn package -q -DskipTests

FROM eclipse-temurin:17-jre-alpine
COPY --from=build /app/target/*.jar /app.jar
COPY --from=build /app/target/dependency/* /libs/
EXPOSE 9103
CMD ["java", "-cp", "/app.jar:/libs/*", "com.example.EchoAgent", "--port", "9103"]
