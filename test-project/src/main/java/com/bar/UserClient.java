package com.bar;

import com.foo.Repository;
import com.foo.Person;

public class UserClient {
    private Repository repo;

    public Person findUser(int id) {
        return repo.findById(id);
    }
}
