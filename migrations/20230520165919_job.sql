-- Add migration script here
CREATE TABLE jobs (
    id bigserial not null primary key,
    sitename varchar not null,
    workorder varchar not null,
    address varchar not null,
    date date not null
);