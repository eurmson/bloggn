CREATE TABLE profile (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    name TEXT NOT NULL,
    role TEXT NOT NULL,
    bio TEXT NOT NULL
);

INSERT INTO profile (id, name, role, bio) VALUES (
    1,
    'Ethan Urmson',
    'Developer & Creator',
    'I explore the intersection of technology and everyday life. By day, I build software; by night, I experiment in the kitchen and write about my journey.'
);
