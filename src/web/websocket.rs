use super::*;

pub(in crate::web) async fn session_events(
    State(state): State<Arc<Shared>>,
    Query(query): Query<StateQuery>,
) -> ApiResult<
    Sse<impl tokio_stream::Stream<Item = std::result::Result<Event, std::convert::Infallible>>>,
> {
    let current = session(&state, &query.id)?;
    let updates = BroadcastStream::new(current.events.subscribe())
        .filter_map(|item| item.ok())
        .map(|data| Ok(Event::default().data(data)));
    let stream = tokio_stream::once(Ok(Event::default().data("connected"))).chain(updates);
    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}
