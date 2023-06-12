FROM rust:1.70 AS build

COPY . /code/

WORKDIR /code

RUN cargo build --release

FROM debian:bullseye-slim AS run

RUN mkdir /app

COPY --from=build /code/target/release/app /app/

WORKDIR /app 

CMD [ "/app/app" ]