<div>
    <h1>{{ $user->getName() }}</h1>
    @foreach([1, 2] as $n)
        <span>{{ $n }}</span>
    @endforeach
</div>
