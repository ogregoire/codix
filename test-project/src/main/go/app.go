package main

type Repository interface {
	Save(item Item)
	Find(id int) Item
}

type Item struct {
	Name string
	ID   int
}

type Service struct {
	repo Repository
}

func (s *Service) Process() {
	s.repo.Save(Item{})
}

func NewService(r Repository) *Service {
	return &Service{repo: r}
}
