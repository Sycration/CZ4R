FROM rust:bookworm AS build

COPY . /code/

WORKDIR /code

RUN apt-get update
RUN apt-get install cmake -y

RUN cargo --version

RUN cargo build --release

FROM debian:bookworm-slim AS run

RUN apt-get update
RUN apt-get install ca-certificates sqlite3 -y

RUN mkdir /app

COPY --from=build /code/target/release/cz4r /app/

WORKDIR /app 

RUN mkdir /database
RUN chmod 777 /database

CMD [ "/app/cz4r" ]