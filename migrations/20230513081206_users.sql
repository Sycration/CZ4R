-- Add migration script here
create table users (
    id bigserial not null primary key,
    name varchar(100) not null,
    hash varchar(100) not null,
    admin boolean not null
)