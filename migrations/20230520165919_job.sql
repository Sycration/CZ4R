-- Add migration script here
CREATE TABLE jobs (
    id integer not null primary key autoincrement,
    sitename varchar not null,
    workorder varchar not null,
    address varchar not null,
    date date not null
);