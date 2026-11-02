FROM rust:1.96-bookworm AS build

COPY . /code/

WORKDIR /code

RUN apt-get update && apt-get install -y --no-install-recommends cmake git && rm -rf /var/lib/apt/lists/*

RUN cargo --version

RUN cargo build --release

FROM debian:bookworm-slim AS run

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates sqlite3 && rm -rf /var/lib/apt/lists/*

RUN mkdir /app

COPY --from=build /code/target/release/cz4r /app/

WORKDIR /app 

RUN mkdir /database
RUN chmod 777 /database

CMD [ "/app/cz4r" ]
