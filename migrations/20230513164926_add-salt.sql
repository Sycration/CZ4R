-- Add migration script here
drop table users;

create table users (
    id integer not null primary key autoincrement,
    name varchar(100) not null,
    hash varchar(100) not null,
    salt varchar(100) not null,
    admin boolean not null,

        unique(name)
)