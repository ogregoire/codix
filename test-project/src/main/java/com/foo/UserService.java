package com.foo;

public class UserService implements Repository {
    private Repository repo;

    public void save(Person p) {
        repo.save(p);
    }

    public Person findById(int id) {
        return repo.findById(id);
    }
}
