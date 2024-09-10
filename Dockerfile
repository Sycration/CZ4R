FROM rust:bullseye AS build

COPY . /code/

WORKDIR /code

RUN apt-get update
RUN apt-get install cmake -y

RUN cargo build --release

FROM debian:bullseye-slim AS run

RUN mkdir /app

COPY --from=build /code/target/release/cz4r /app/

WORKDIR /app 

RUN mkdir /database
RUN chmod 777 /database

CMD [ "/app/cz4r" ]