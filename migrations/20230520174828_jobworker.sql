-- Add migration script here
CREATE TABLE jobworkers (
    job integer not null references jobs(id) ,
    worker integer not null references users(id) ,
    signin varchar(100),
    signout varchar(100),
    extraexpcents int not null default 0,
    notes text not null default ''    
);