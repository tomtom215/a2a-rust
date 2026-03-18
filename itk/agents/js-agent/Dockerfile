FROM node:22-slim
WORKDIR /app
COPY package.json .
RUN npm install --production
COPY index.js .
EXPOSE 9101
CMD ["node", "index.js", "--port", "9101"]
