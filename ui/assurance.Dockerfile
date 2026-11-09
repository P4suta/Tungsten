FROM node:26.13.0-alpine@sha256:8996bb5821d62a8fd9bb27c1cb07462f3fdce6e47d640c6a881ead0b064ce216
WORKDIR /project/ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci --ignore-scripts
COPY ui/assurance/package.json ui/assurance/package-lock.json assurance/
RUN npm --prefix assurance ci --ignore-scripts
COPY ui/ ./
RUN npm run build
ENTRYPOINT ["timeout", "-s", "TERM", "-k", "10", "890"]
CMD ["npm", "run", "test:mutations"]
