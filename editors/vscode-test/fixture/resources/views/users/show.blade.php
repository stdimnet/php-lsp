@section('content')
<div>
    <h1>{{ $user->getName() }}</h1>
    @foreach([1, 2] as $n)
        <span>{{ $n }} {{ $loop->index }}</span>
    @endforeach
</div>
@endsection
