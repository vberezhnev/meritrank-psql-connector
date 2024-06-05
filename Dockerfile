FROM rust:1.75.0-alpine as compile
WORKDIR /usr/project
COPY . .
RUN apk add cmake make gcc clang15 musl-dev postgresql16 postgresql16-dev openssl-dev
ENV RUSTFLAGS="-Ctarget-feature=-crt-static"
RUN cargo install --locked --version 0.11.4 cargo-pgrx
RUN cargo pgrx init --pg16 pg_config
RUN cargo pgrx package
RUN ls -la target/release/pgmer2-pg16/usr/share/postgresql16/extension

FROM postgres:16-alpine
ENV MERITRANK_SERVICE_URL=tcp://127.0.0.1:10234
ENV MERITRANK_RECV_TIMEOUT_MSEC=10000
# FIXME
ENV POSTGRES_HOST_AUTH_METHOD=trust

COPY --from=compile /usr/project/target/release/pgmer2-pg16/usr/lib/postgresql16/pgmer2.so /usr/local/lib/postgresql/pgmer2.so
COPY --from=compile /usr/project/target/release/pgmer2-pg16/usr/share/postgresql16/extension /usr/local/share/postgresql/extension
COPY 20_pgmer2.sh /docker-entrypoint-initdb.d/20_pgmer2.sh
