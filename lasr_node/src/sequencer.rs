// This struct needs to take in new transactions 
// and decide if they are dependent on pending transactions
// and if so, scheduler them to run upon the completion of 
// pending transactions, if not, schedule them to run immediately.
pub struct Sequencer;
