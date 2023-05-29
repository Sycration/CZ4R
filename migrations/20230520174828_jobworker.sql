-- Add migration script here
CREATE TABLE jobworkers (
    job bigserial not null references jobs(id),
    worker bigserial not null references users(id),
    signin time without time zone,
    signout time without time zone,
    extraexpcents int not null default 0,
    notes text not null default ''    
);